use std::fmt::Debug;
use std::marker::PhantomData;

#[cfg(not(test))]
use multiversx_sc::api::ErrorApiImpl;
use multiversx_sc::{
    api::StorageMapperApi,
    storage::{
        mappers::{SingleValueMapper, StorageMapper, VecMapper},
        StorageKey,
    },
    types::heap::BoxedBytes,
};
use multiversx_sc_codec::{
    self as codec,
    derive::{TopDecode, TopEncode},
    NestedDecode, NestedEncode, TopDecode, TopEncode,
};
use std::ops::Bound;

use crate::dex::collection_helpers::{StorageRef, StorageRefPairIter};
use crate::dex::{KeyAt, Map, MapRemoveKey, OrderedMap, Result};

// Suffixes to use for the storage
const TREE_VEC_SUFFIX: &[u8] = b".tree";
const ROOT_SUFFIX: &[u8] = b".root";
const VALUE_SUFFIX: &[u8] = b".value";

/// Key trait to simplify types signatures
pub trait OrderedMapKeyTrait:
    Ord + Clone + Debug + TopEncode + TopDecode + NestedEncode + NestedDecode + 'static
{
}
impl<T> OrderedMapKeyTrait for T where
    T: Ord + Clone + Debug + TopEncode + TopDecode + NestedEncode + NestedDecode + 'static
{
}

/// Value trait to simplify types signatures
pub trait OrderedMapValueTrait: NestedEncode + NestedDecode + 'static {}
impl<T> OrderedMapValueTrait for T where T: NestedEncode + NestedDecode + 'static {}

/// `TreeMap` based on AVL-tree
/// Inspired by Near `TreeMap` with approaches from native `MultiverseX` collections
///
/// Runtime complexity (worst case):
/// - `get`/`contains_key`:     O(1) - `UnorderedMap` lookup
/// - `insert`/`remove`:        O(log(N))
/// - `min`/`max`:              O(log(N))
/// - `above`/`below`:          O(log(N))
/// - `range` of K elements:    O(Klog(N))
///
pub struct StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    _phantom_value: PhantomData<V>,
    // Base key to make element keys from
    base_key: StorageKey<SA>,
    // Root element index in a tree vector
    root: SingleValueMapper<SA, usize>,
    // Tree vec mapper
    tree: VecMapper<SA, Node<K>>,
}

/// Structure to store values. We need an additional wrapper to store `zeroed` values, because
/// `MultiverseX` doesn't support storing zeroes as top values
#[derive(TopEncode, TopDecode)]
pub struct Value<V: OrderedMapValueTrait>(V);

#[derive(Clone, Debug, TopEncode, TopDecode)]
pub struct Node<K>
where
    K: OrderedMapKeyTrait,
{
    id: usize,
    key: K,             // key stored in a node
    lft: Option<usize>, // left link of a node
    rgt: Option<usize>, // right link of a node
    ht: usize,          // height of a subtree at a node
}

impl<K> Node<K>
where
    K: OrderedMapKeyTrait,
{
    fn of(id: usize, key: K) -> Self {
        Self {
            id,
            key,
            lft: None,
            rgt: None,
            ht: 1,
        }
    }
}

impl<SA, K, V> StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    /// Makes a new, empty `TreeMap`
    pub fn new(storage_key: &[u8]) -> Self {
        let base_key = StorageKey::new(storage_key);
        let root = Self::create_root_mapper(&base_key);
        let tree = Self::create_tree_mapper(&base_key);

        Self {
            _phantom_value: PhantomData,
            base_key,
            root,
            tree,
        }
    }

    /// Returns the number of elements in the tree, also referred to as its size.
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    /// Clears the tree, removing all elements.
    pub fn clear(&mut self) {
        // Note: indices in MultiverseX vec start from `1`
        self.root.set(1);

        for n in self.tree.iter() {
            self.create_value_mapper(&n.key).clear();
        }
        self.tree.clear();
    }

    fn node(&self, id: usize) -> Option<Node<K>> {
        // Note: indices in MultiverseX vec start from `1`
        if id > 0 && id <= self.tree.len() {
            Some(self.tree.get(id))
        } else {
            None
        }
    }

    fn save(&mut self, node: &Node<K>) {
        // Note: indices in MultiverseX vec start from `1`
        if node.id <= self.len() {
            self.tree.set(node.id, node);
        } else {
            self.tree.push(node);
        }
    }

    /// Returns true if the map contains a given key.
    pub fn contains_key(&self, key: &K) -> bool {
        !self.create_value_mapper(key).is_empty()
    }

    /// Returns the value corresponding to the key.
    pub fn get(&self, key: &K) -> Option<V> {
        let value_mapper = self.create_value_mapper(key);

        if value_mapper.is_empty() {
            None
        } else {
            Some(value_mapper.get().0)
        }
    }

    /// Inserts a key-value pair into the tree.
    /// If the tree did not have this key present, `None` is returned. Otherwise returns
    /// a value. Note, the keys that have the same value are undistinguished by
    /// the implementation.
    pub fn insert(&mut self, key: &K, val: V) -> Option<V> {
        let root = self.root.get();
        let new_root = self.insert_at(root, self.tree.len() + 1, key);

        if self.contains_key(key) {
            let result = self.get(key);

            self.create_value_mapper(key).update(|stored_value| {
                *stored_value = Value(val);
            });

            result
        } else {
            self.root.set(new_root);
            self.create_value_mapper(key).set(Value(val));

            None
        }
    }

    /// Removes a key from the tree, returning the value at the key if the key was previously in the
    /// tree.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if self.contains_key(key) {
            let new_root = self.do_remove(key);
            self.root.set(new_root);

            let value_mapper = self.create_value_mapper(key);

            let ret = value_mapper.get();
            value_mapper.clear();
            Some(ret.0)
        } else {
            // no such key, nothing to do
            None
        }
    }

    /// Returns the smallest stored key from the tree
    pub fn min(&self) -> Option<K> {
        let root = self.root.get();

        self.min_at(root, root).map(|(n, _)| n.key)
    }

    /// Returns the largest stored key from the tree
    pub fn max(&self) -> Option<K> {
        let root = self.root.get();

        self.max_at(root, root).map(|(n, _)| n.key)
    }

    /// Returns the smallest key that is strictly greater than key given as the parameter
    pub fn higher(&self, key: &K) -> Option<K> {
        self.above_at(self.root.get(), key)
    }

    /// Returns the largest key that is strictly less than key given as the parameter
    pub fn lower(&self, key: &K) -> Option<K> {
        self.below_at(self.root.get(), key)
    }

    /// Returns the smallest key that is greater or equal to key given as the parameter
    pub fn ceil_key(&self, key: &K) -> Option<K> {
        if self.contains_key(key) {
            Some(key.clone())
        } else {
            self.higher(key)
        }
    }

    /// Returns the largest key that is less or equal to key given as the parameter
    pub fn floor_key(&self, key: &K) -> Option<K> {
        if self.contains_key(key) {
            Some(key.clone())
        } else {
            self.lower(key)
        }
    }

    /// Iterate all entries in ascending order: min to max, both inclusive
    pub fn iter(&self) -> Cursor<'_, SA, K, V> {
        Cursor::asc(self)
    }

    /// Iterate entries in ascending order: given key (exclusive) to max (inclusive)
    pub fn iter_from(&self, key: &K) -> Cursor<'_, SA, K, V> {
        Cursor::asc_from(self, key)
    }

    /// Iterate all entries in descending order: max to min, both inclusive
    pub fn iter_rev(&self) -> Cursor<'_, SA, K, V> {
        Cursor::desc(self)
    }

    /// Iterate entries in descending order: given key (exclusive) to min (inclusive)
    pub fn iter_rev_from(&self, key: &K) -> Cursor<'_, SA, K, V> {
        Cursor::desc_from(self, key)
    }

    fn signal_error(message: &str) -> ! {
        #[cfg(not(test))]
        SA::error_api_impl().signal_error(message.as_bytes());
        #[cfg(test)]
        panic!("{}", message);
    }

    /// Iterate entries in ascending order according to specified bounds.
    ///
    /// # Panics
    ///
    /// Panics if range start > end.
    /// Panics if range start == end and both bounds are Excluded.
    pub fn range(&self, r: (Bound<K>, Bound<K>)) -> Cursor<'_, SA, K, V> {
        let (lo, hi) = match r {
            (Bound::Included(a), Bound::Included(b)) if a > b => {
                Self::signal_error("Invalid range")
            }
            (Bound::Excluded(a), Bound::Included(b)) if a > b => {
                Self::signal_error("Invalid range")
            }
            (Bound::Included(a), Bound::Excluded(b)) if a > b => {
                Self::signal_error("Invalid range")
            }
            (Bound::Excluded(a), Bound::Excluded(b)) if a >= b => {
                Self::signal_error("Invalid range")
            }
            (lo, hi) => (lo, hi),
        };

        Cursor::range(self, lo, hi)
    }

    /// Helper function which creates a [`Vec<(K, V)>`] of all items in the [`TreeMap`].
    /// This function collects elements from [`TreeMap::iter`].
    pub fn to_vec(&self) -> Vec<(K, V)> {
        self.iter().collect()
    }

    //
    // Internal utilities
    //

    /// Make root mapper out of the base key
    fn create_root_mapper(base_key: &StorageKey<SA>) -> SingleValueMapper<SA, usize> {
        let mut root_key = base_key.clone();
        root_key.append_bytes(ROOT_SUFFIX);

        let mapper = SingleValueMapper::new(root_key);

        // Empy root key means just created map. Init with 1
        // Note: indices in MultiverseX vec start from `1`
        mapper.set_if_empty(1);

        mapper
    }

    /// Make tree mapper out of the base key
    fn create_tree_mapper(base_key: &StorageKey<SA>) -> VecMapper<SA, Node<K>> {
        let mut tree_key = base_key.clone();
        tree_key.append_bytes(TREE_VEC_SUFFIX);

        VecMapper::new(tree_key)
    }

    /// Make value mapper for a given key
    fn create_value_mapper(&self, key: &K) -> SingleValueMapper<SA, Value<V>> {
        let mut value_key = self.base_key.clone();
        value_key.append_item(&key);
        value_key.append_bytes(VALUE_SUFFIX);

        SingleValueMapper::new(value_key)
    }

    /// Returns (node, parent node) of left-most lower (min) node starting from given node `at`.
    /// As `min_at` only traverses the tree down, if a node `at` is the minimum node in a subtree,
    /// its parent must be explicitly provided in advance.
    fn min_at(&self, mut at: usize, p: usize) -> Option<(Node<K>, Node<K>)> {
        let mut parent: Option<Node<K>> = self.node(p);
        loop {
            let node = self.node(at);
            match node.as_ref().and_then(|n| n.lft) {
                Some(lft) => {
                    at = lft;
                    parent = node;
                }
                None => {
                    return node.and_then(|n| parent.map(|p| (n, p)));
                }
            }
        }
    }

    /// Returns (node, parent node) of right-most lower (max) node starting from given node `at`.
    /// As `min_at` only traverses the tree down, if a node `at` is the minimum node in a subtree,
    /// its parent must be explicitly provided in advance.
    fn max_at(&self, mut at: usize, p: usize) -> Option<(Node<K>, Node<K>)> {
        let mut parent: Option<Node<K>> = self.node(p);
        loop {
            let node = self.node(at);
            match node.as_ref().and_then(|n| n.rgt) {
                Some(rgt) => {
                    parent = node;
                    at = rgt;
                }
                None => {
                    return node.and_then(|n| parent.map(|p| (n, p)));
                }
            }
        }
    }

    fn above_at(&self, mut at: usize, key: &K) -> Option<K> {
        let mut seen: Option<K> = None;
        loop {
            let node = self.node(at);
            match node.as_ref().map(|n| &n.key) {
                Some(k) => {
                    if k.le(key) {
                        match node.and_then(|n| n.rgt) {
                            Some(rgt) => at = rgt,
                            None => break,
                        }
                    } else {
                        seen = Some(k.clone());
                        match node.and_then(|n| n.lft) {
                            Some(lft) => at = lft,
                            None => break,
                        }
                    }
                }
                None => break,
            }
        }
        seen
    }

    fn below_at(&self, mut at: usize, key: &K) -> Option<K> {
        let mut seen: Option<K> = None;
        loop {
            let node = self.node(at);
            match node.as_ref().map(|n| &n.key) {
                Some(k) => {
                    if k.lt(key) {
                        seen = Some(k.clone());
                        match node.and_then(|n| n.rgt) {
                            Some(rgt) => at = rgt,
                            None => break,
                        }
                    } else {
                        match node.and_then(|n| n.lft) {
                            Some(lft) => at = lft,
                            None => break,
                        }
                    }
                }
                None => break,
            }
        }
        seen
    }

    fn insert_at(&mut self, at: usize, id: usize, key: &K) -> usize {
        match self.node(at) {
            None => {
                self.save(&Node::<K>::of(id, key.clone()));
                at
            }
            Some(mut node) => {
                if key.eq(&node.key) {
                    at
                } else {
                    if key.lt(&node.key) {
                        let idx = match node.lft {
                            Some(lft) => self.insert_at(lft, id, key),
                            None => self.insert_at(id, id, key),
                        };
                        node.lft = Some(idx);
                    } else {
                        let idx = match node.rgt {
                            Some(rgt) => self.insert_at(rgt, id, key),
                            None => self.insert_at(id, id, key),
                        };
                        node.rgt = Some(idx);
                    };

                    self.update_height(&mut node);
                    self.enforce_balance(&mut node)
                }
            }
        }
    }

    // Calculate and save the height of a subtree at node `at`:
    // height[at] = 1 + max(height[at.L], height[at.R])
    fn update_height(&mut self, node: &mut Node<K>) {
        let lft = node
            .lft
            .and_then(|id| self.node(id).map(|n| n.ht))
            .unwrap_or_default();
        let rgt = node
            .rgt
            .and_then(|id| self.node(id).map(|n| n.ht))
            .unwrap_or_default();

        node.ht = 1 + std::cmp::max(lft, rgt);
        self.save(node);
    }

    // Balance = difference in heights between left and right subtrees at given node.
    fn get_balance(&self, node: &Node<K>) -> i64 {
        let lht = node
            .lft
            .and_then(|id| self.node(id).map(|n| n.ht))
            .unwrap_or_default();
        let rht = node
            .rgt
            .and_then(|id| self.node(id).map(|n| n.ht))
            .unwrap_or_default();

        lht as i64 - rht as i64
    }

    // Left rotation of an AVL subtree with at node `at`.
    // New root of subtree is returned, caller is responsible for updating proper link from parent.
    fn rotate_left(&mut self, node: &mut Node<K>) -> usize {
        let mut lft = node.lft.and_then(|id| self.node(id)).unwrap();
        let lft_rgt = lft.rgt;

        // at.L = at.L.R
        node.lft = lft_rgt;

        // at.L.R = at
        lft.rgt = Some(node.id);

        // at = at.L
        self.update_height(node);
        self.update_height(&mut lft);

        lft.id
    }

    // Right rotation of an AVL subtree at node in `at`.
    // New root of subtree is returned, caller is responsible for updating proper link from parent.
    fn rotate_right(&mut self, node: &mut Node<K>) -> usize {
        let mut rgt = node.rgt.and_then(|id| self.node(id)).unwrap();
        let rgt_lft = rgt.lft;

        // at.R = at.R.L
        node.rgt = rgt_lft;

        // at.R.L = at
        rgt.lft = Some(node.id);

        // at = at.R
        self.update_height(node);
        self.update_height(&mut rgt);

        rgt.id
    }

    // Check balance at a given node and enforce it if necessary with respective rotations.
    fn enforce_balance(&mut self, node: &mut Node<K>) -> usize {
        let balance = self.get_balance(node);
        if balance > 1 {
            let mut lft = node.lft.and_then(|id| self.node(id)).unwrap();
            if self.get_balance(&lft) < 0 {
                let rotated = self.rotate_right(&mut lft);
                node.lft = Some(rotated);
            }
            self.rotate_left(node)
        } else if balance < -1 {
            let mut rgt = node.rgt.and_then(|id| self.node(id)).unwrap();
            if self.get_balance(&rgt) > 0 {
                let rotated = self.rotate_left(&mut rgt);
                node.rgt = Some(rotated);
            }
            self.rotate_right(node)
        } else {
            node.id
        }
    }

    // Returns (node, parent node) for a node that holds the `key`.
    // For root node, same node is returned for node and parent node.
    fn lookup_at(&self, mut at: usize, key: &K) -> Option<(Node<K>, Node<K>)> {
        let mut p: Node<K> = self.node(at).unwrap();
        while let Some(node) = self.node(at) {
            if node.key.eq(key) {
                return Some((node, p));
            } else if node.key.lt(key) {
                match node.rgt {
                    Some(rgt) => {
                        p = node;
                        at = rgt;
                    }
                    None => break,
                }
            } else {
                match node.lft {
                    Some(lft) => {
                        p = node;
                        at = lft;
                    }
                    None => break,
                }
            }
        }
        None
    }

    // Navigate from root to node holding `key` and backtrace back to the root
    // enforcing balance (if necessary) along the way.
    fn check_balance(&mut self, at: usize, key: &K) -> usize {
        match self.node(at) {
            Some(mut node) => {
                if !node.key.eq(key) {
                    if node.key.gt(key) {
                        if let Some(l) = node.lft {
                            let id = self.check_balance(l, key);
                            node.lft = Some(id);
                        }
                    } else if let Some(r) = node.rgt {
                        let id = self.check_balance(r, key);
                        node.rgt = Some(id);
                    }
                }
                self.update_height(&mut node);
                self.enforce_balance(&mut node)
            }
            None => at,
        }
    }

    // Node holding the key is not removed from the tree - instead the substitute node is found,
    // the key is copied to 'removed' node from substitute node, and then substitute node gets
    // removed from the tree.
    //
    // The substitute node is either:
    // - right-most (max) node of the left subtree (containing smaller keys) of node holding `key`
    // - or left-most (min) node of the right subtree (containing larger keys) of node holding `key`
    //
    fn do_remove(&mut self, key: &K) -> usize {
        let root = self.root.get();

        // r_node - node containing key of interest
        // p_node - immediate parent node of r_node
        let Some((mut r_node, mut p_node)) = self.lookup_at(root, key) else {
            return root; // cannot remove a missing key, no changes to the tree needed
        };

        let lft_opt = r_node.lft;
        let rgt_opt = r_node.rgt;

        if lft_opt.is_none() && rgt_opt.is_none() {
            // remove leaf
            if p_node.key.lt(key) {
                p_node.rgt = None;
            } else {
                p_node.lft = None;
            }
            self.update_height(&mut p_node);

            self.swap_with_last(r_node.id);

            // removing node might have caused a imbalance - balance the tree up to the root,
            // starting from lowest affected key - the parent of a leaf node in this case
            // Note: do not use cached root here. It's been updated
            self.check_balance(self.root.get(), &p_node.key)
        } else {
            // non-leaf node, select subtree to proceed with
            let b = self.get_balance(&r_node);
            if b >= 0 {
                // proceed with left subtree
                let lft = lft_opt.unwrap();

                // k - max key from left subtree
                // n - node that holds key k, p - immediate parent of n
                let (n, mut p) = self.max_at(lft, r_node.id).unwrap();
                let k = n.key.clone();

                if p.rgt.as_ref().map(|&id| id == n.id).unwrap_or_default() {
                    // n is on right link of p
                    p.rgt = n.lft;
                } else {
                    // n is on left link of p
                    p.lft = n.lft;
                }

                self.update_height(&mut p);

                if r_node.id == p.id {
                    // r_node.id and p.id can overlap on small trees (2 levels, 2-3 nodes)
                    // that leads to nasty lost update of the key, refresh below fixes that
                    r_node = self.node(r_node.id).unwrap();
                }
                r_node.key = k;
                self.save(&r_node);

                self.swap_with_last(n.id);

                // removing node might have caused an imbalance - balance the tree up to the root,
                // starting from the lowest affected key (max key from left subtree in this case)
                // Note: do not use cached root here. It's been updated
                self.check_balance(self.root.get(), &p.key)
            } else {
                // proceed with right subtree
                let rgt = rgt_opt.unwrap();

                // k - min key from right subtree
                // n - node that holds key k, p - immediate parent of n
                let (n, mut p) = self.min_at(rgt, r_node.id).unwrap();
                let k = n.key.clone();

                if p.lft.map(|id| id == n.id).unwrap_or_default() {
                    // n is on left link of p
                    p.lft = n.rgt;
                } else {
                    // n is on right link of p
                    p.rgt = n.rgt;
                }

                self.update_height(&mut p);

                if r_node.id == p.id {
                    // r_node.id and p.id can overlap on small trees (2 levels, 2-3 nodes)
                    // that leads to nasty lost update of the key, refresh below fixes that
                    r_node = self.node(r_node.id).unwrap();
                }
                r_node.key = k;
                self.save(&r_node);

                self.swap_with_last(n.id);

                // removing node might have caused a imbalance - balance the tree up to the root,
                // starting from the lowest affected key (min key from right subtree in this case)
                // Note: do not use cached root here. It's been updated
                self.check_balance(self.root.get(), &p.key)
            }
        }
    }

    // Move content of node with id = `len` (parent left or right link, left, right, key, height)
    // to node with given `id`, and remove node `len` (pop the vector of nodes).
    // This ensures that among `n` nodes in the tree, max `id` is `n`, so when new node is inserted,
    // it gets an `id` as its position in the vector.
    fn swap_with_last(&mut self, id: usize) {
        // Note: indices in MultiverseX vec start from `1`
        if id == self.len() {
            // noop: id is already last element in the vector
            self.tree.swap_remove(self.tree.len());
            return;
        }

        let key = self.node(self.len()).map(|n| n.key).unwrap();
        let (mut n, mut p) = self.lookup_at(self.root.get(), &key).unwrap();

        if n.id != p.id {
            if p.lft.map(|id| id == n.id).unwrap_or_default() {
                p.lft = Some(id);
            } else {
                p.rgt = Some(id);
            }
            self.save(&p);
        }

        if self.root.get() == n.id {
            self.root.set(id);
        }

        n.id = id;
        self.save(&n);
        // Note: indices in MultiverseX vec start from `1`
        self.tree.swap_remove(self.tree.len());
    }
}

/// Top encoding implementation
impl<SA, K, V> TopEncode for StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    fn top_encode<O: multiversx_sc_codec::TopEncodeOutput>(
        &self,
        output: O,
    ) -> Result<(), multiversx_sc_codec::EncodeError> {
        self.base_key.to_boxed_bytes().top_encode(output)
    }
}

/// Top decoding implementation
impl<SA, K, V> TopDecode for StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    fn top_decode<I: multiversx_sc_codec::TopDecodeInput>(
        input: I,
    ) -> Result<Self, multiversx_sc_codec::DecodeError> {
        let bytes = BoxedBytes::top_decode(input)?;

        Ok(Self::new(bytes.as_slice()))
    }
}

/// Nested encoding implementation
impl<SA, K, V> NestedEncode for StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    fn dep_encode<O: multiversx_sc_codec::NestedEncodeOutput>(
        &self,
        dest: &mut O,
    ) -> std::result::Result<(), multiversx_sc_codec::EncodeError> {
        self.base_key.to_boxed_bytes().dep_encode(dest)
    }
}

/// Nested decoding implementation
impl<SA, K, V> NestedDecode for StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    fn dep_decode<I: multiversx_sc_codec::NestedDecodeInput>(
        input: &mut I,
    ) -> std::result::Result<Self, multiversx_sc_codec::DecodeError> {
        let bytes = BoxedBytes::dep_decode(input)?;

        Ok(Self::new(bytes.as_slice()))
    }
}

impl<SA, K, V> Map for StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    type Key = K;
    type Value = V;
    type KeyRef<'a> = StorageRef<'a, K> where Self: 'a;
    type ValueRef<'a> = StorageRef<'a, V> where Self: 'a;
    type Iter<'a> = StorageRefPairIter<'a, K, V, Cursor<'a, SA, K, V>> where Self: 'a;

    fn iter(&self) -> Self::Iter<'_> {
        StorageRefPairIter::new(self.iter())
    }

    fn clear(&mut self) {
        self.clear();
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn contains_key(&self, key: &K) -> bool {
        self.contains_key(key)
    }

    fn inspect<R, F: FnOnce(&V) -> R>(&self, key: &K, inspect_fn: F) -> Option<R> {
        self.get(key).map(|value| inspect_fn(&value))
    }

    // Can't match over the value, because Elrond lib doen's export the Entry type
    fn update<R, F: FnOnce(&mut V) -> Result<R>>(
        &mut self,
        key: &K,
        update_fn: F,
    ) -> Option<Result<R>> {
        self.get(key).map(|mut value| {
            let result = update_fn(&mut value);
            self.insert(key, value);
            result
        })
    }

    fn update_or_insert<R, F, U>(&mut self, key: &K, factory_fn: F, update_fn: U) -> Result<R>
    where
        F: FnOnce() -> Result<V>,
        U: FnOnce(&mut V, /* exists */ bool) -> Result<R>,
    {
        let (mut value, exists) = match self.get(key) {
            None => (factory_fn()?, false),
            Some(value) => (value, true),
        };

        let result = update_fn(&mut value, exists);
        self.insert(key, value);
        result
    }

    fn insert(&mut self, key: K, value: V) {
        self.insert(&key, value);
    }
}

impl<SA, K, V> MapRemoveKey for StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    fn remove(&mut self, key: &K) {
        self.remove(key);
    }
}

fn find_key_at<SA, K, V>(map: &StorageOrderedMap<SA, K, V>, at: KeyAt<&K>) -> Option<K>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    match at {
        KeyAt::Min => map.min(),
        KeyAt::Max => map.max(),
        KeyAt::Above(key) => map.higher(key),
        KeyAt::Below(key) => map.lower(key),
    }
}

impl<SA, K, V> OrderedMap for StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    fn inspect_at<R, F: FnOnce(&K, &V) -> R>(&self, at: KeyAt<&K>, inspect_fn: F) -> Option<R> {
        find_key_at(self, at).and_then(|key| self.inspect(&key, |value| inspect_fn(&key, value)))
    }

    fn update_at<R, F: FnOnce(&K, &mut V) -> Result<R>>(
        &mut self,
        at: KeyAt<&K>,
        update_fn: F,
    ) -> Option<Result<R>> {
        find_key_at(self, at).and_then(|key| self.update(&key, |value| update_fn(&key, value)))
    }
}

impl<SA, K, V> Debug for StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeMap")
            .field("root", &self.root.get())
            .field("tree", &self.tree.iter().collect::<Vec<Node<K>>>())
            .finish()
    }
}

impl<'a, SA, K, V> IntoIterator for &'a StorageOrderedMap<SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    type Item = (K, V);
    type IntoIter = Cursor<'a, SA, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        Cursor::asc(self)
    }
}

impl<SA, K, V> Iterator for Cursor<'_, SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        <Self as Iterator>::nth(self, 0)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Constrains max count. Not worth it to cause storage reads to make this more accurate.
        (0, Some(self.map.len()))
    }

    fn count(mut self) -> usize {
        // Because this Cursor allows for bounded/starting from a key, there is no way of knowing
        // how many elements are left to iterate without loading keys in order. This could be
        // optimized in the case of a standard iterator by having a separate type, but this would
        // be a breaking change, so there will be slightly more reads than necessary in this case.
        let mut count = 0;
        while self.key.is_some() {
            count += 1;
            self.progress_key();
        }
        count
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        for _ in 0..n {
            // Skip over elements not iterated over to get to `nth`. This avoids loading values
            // from storage.
            self.progress_key();
        }

        let key = self.progress_key()?;
        let value = self.map.get(&key)?;

        Some((key, value))
    }

    fn last(mut self) -> Option<Self::Item> {
        if self.asc && matches!(self.hi, Bound::Unbounded) {
            self.map
                .max()
                .and_then(|k| self.map.get(&k).map(|v| (k, v)))
        } else if !self.asc && matches!(self.lo, Bound::Unbounded) {
            self.map
                .min()
                .and_then(|k| self.map.get(&k).map(|v| (k, v)))
        } else {
            // Cannot guarantee what the last is within the range, must load keys until last.
            let key = core::iter::from_fn(|| self.progress_key()).last();
            key.and_then(|k| self.map.get(&k).map(|v| (k, v)))
        }
    }
}

impl<SA, K, V> std::iter::FusedIterator for Cursor<'_, SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
}

fn fits<K: Ord>(key: &K, lo: &Bound<K>, hi: &Bound<K>) -> bool {
    (match lo {
        Bound::Included(ref x) => key >= x,
        Bound::Excluded(ref x) => key > x,
        Bound::Unbounded => true,
    }) && (match hi {
        Bound::Included(ref x) => key <= x,
        Bound::Excluded(ref x) => key < x,
        Bound::Unbounded => true,
    })
}

pub struct Cursor<'a, SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    asc: bool,
    lo: Bound<K>,
    hi: Bound<K>,
    key: Option<K>,
    map: &'a StorageOrderedMap<SA, K, V>,
}

impl<'a, SA, K, V> Cursor<'a, SA, K, V>
where
    SA: StorageMapperApi,
    K: OrderedMapKeyTrait,
    V: OrderedMapValueTrait,
{
    fn asc(map: &'a StorageOrderedMap<SA, K, V>) -> Self {
        let key: Option<K> = map.min();
        Self {
            asc: true,
            key,
            lo: Bound::Unbounded,
            hi: Bound::Unbounded,
            map,
        }
    }

    fn asc_from(map: &'a StorageOrderedMap<SA, K, V>, key: &K) -> Self {
        let key = map.higher(key);
        Self {
            asc: true,
            key,
            lo: Bound::Unbounded,
            hi: Bound::Unbounded,
            map,
        }
    }

    fn desc(map: &'a StorageOrderedMap<SA, K, V>) -> Self {
        let key: Option<K> = map.max();
        Self {
            asc: false,
            key,
            lo: Bound::Unbounded,
            hi: Bound::Unbounded,
            map,
        }
    }

    fn desc_from(map: &'a StorageOrderedMap<SA, K, V>, key: &K) -> Self {
        let key = map.lower(key);
        Self {
            asc: false,
            key,
            lo: Bound::Unbounded,
            hi: Bound::Unbounded,
            map,
        }
    }

    fn range(map: &'a StorageOrderedMap<SA, K, V>, lo: Bound<K>, hi: Bound<K>) -> Self {
        let key = match &lo {
            Bound::Included(k) if map.contains_key(k) => Some(k.clone()),
            Bound::Included(k) | Bound::Excluded(k) => map.higher(k),
            Bound::Unbounded => None,
        };
        let key = key.filter(|k| fits(k, &lo, &hi));

        Self {
            asc: true,
            key,
            lo,
            hi,
            map,
        }
    }

    /// Progresses the key one index, will return the previous key
    fn progress_key(&mut self) -> Option<K> {
        let new_key = self
            .key
            .as_ref()
            .and_then(|k| {
                if self.asc {
                    self.map.higher(k)
                } else {
                    self.map.lower(k)
                }
            })
            .filter(|k| fits(k, &self.lo, &self.hi));
        core::mem::replace(&mut self.key, new_key)
    }
}

// TODO:
// 1. Move this to `tests` when merged with main
// 2. Reduce number of tests (merge them) to reduce blockchain storage initialization overhead
#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::needless_pass_by_value
)]
mod tests {
    use super::*;

    extern crate rand;
    use self::rand::RngCore;
    use multiversx_sc_scenario::testing_framework::BlockchainStateWrapper;
    use multiversx_sc_scenario::DebugApi;
    use quickcheck::QuickCheck;
    use rand::Rng;
    use std::collections::BTreeMap;
    use std::collections::HashSet;

    /// Initializes state mock
    fn setup_env() -> BlockchainStateWrapper {
        let wrapper = BlockchainStateWrapper::new();

        // Add transaction context on top, which contains managed memory API
        let _ = DebugApi::dummy();

        wrapper
    }

    /// Return height of the tree - number of nodes on the longest path starting from the root node.
    fn height<SA, K, V>(tree: &StorageOrderedMap<SA, K, V>) -> usize
    where
        SA: StorageMapperApi,
        K: OrderedMapKeyTrait,
        V: OrderedMapValueTrait,
    {
        tree.node(tree.root.get()).map(|n| n.ht).unwrap_or_default()
    }

    fn random(n: usize) -> Vec<u32> {
        let mut rng = rand::thread_rng();
        let mut vec = Vec::with_capacity(n);
        (0..n).for_each(|_| {
            vec.push(rng.next_u32() % 1000);
        });
        vec
    }

    fn next_trie_id() -> Vec<u8> {
        let mut rng = rand::thread_rng();
        let mut vec = Vec::with_capacity(10);
        (0..10).for_each(|_| {
            vec.push(rng.gen());
        });
        vec
    }

    fn log2(x: f64) -> f64 {
        std::primitive::f64::log(x, 2.0f64)
    }

    fn max_tree_height(n: usize) -> usize {
        // h <= C * log2(n + D) + B
        // where:
        // C =~ 1.440, D =~ 1.065, B =~ 0.328
        // (source: https://en.wikipedia.org/wiki/AVL_tree)
        const B: f64 = -0.328;
        const C: f64 = 1.440;
        const D: f64 = 1.065;

        let h = C * log2(n as f64 + D) + B;
        h.ceil() as usize
    }

    #[test]
    fn test_empty() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u8, u8> = StorageOrderedMap::new(&next_trie_id());
        assert_eq!(map.len(), 0);
        assert_eq!(height(&map), 0);
        assert_eq!(map.get(&42), None);
        assert!(!map.contains_key(&42));
        assert_eq!(map.min(), None);
        assert_eq!(map.max(), None);
        assert_eq!(map.lower(&42), None);
        assert_eq!(map.higher(&42), None);
    }

    #[test]
    fn test_insert_3_rotate_l_l() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, i32, i32> =
            StorageOrderedMap::new(&next_trie_id());
        assert_eq!(height(&map), 0);

        map.insert(&3, 3);
        assert_eq!(height(&map), 1);

        map.insert(&2, 2);
        assert_eq!(height(&map), 2);

        map.insert(&1, 1);
        assert_eq!(height(&map), 2);

        let root = map.root.get();
        assert_eq!(root, 2);
        assert_eq!(map.node(root).map(|n| n.key), Some(2));

        map.clear();
    }

    #[test]
    fn test_insert_3_rotate_r_r() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, i32, i32> =
            StorageOrderedMap::new(&next_trie_id());
        assert_eq!(height(&map), 0);

        map.insert(&1, 1);
        assert_eq!(height(&map), 1);

        map.insert(&2, 2);
        assert_eq!(height(&map), 2);

        map.insert(&3, 3);

        let root = map.root.get();
        assert_eq!(root, 2);
        assert_eq!(map.node(root).map(|n| n.key), Some(2));
        assert_eq!(height(&map), 2);

        map.clear();
    }

    #[test]
    fn test_insert_lookup_n_asc() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, i32, i32> =
            StorageOrderedMap::new(&next_trie_id());

        let n: usize = 30;
        let cases = (0..2 * (n as i32)).collect::<Vec<i32>>();

        let mut counter = 0;
        for k in &cases {
            if *k % 2 == 0 {
                counter += 1;
                map.insert(k, counter);
            }
        }

        counter = 0;
        for k in &cases {
            if *k % 2 == 0 {
                counter += 1;
                assert_eq!(map.get(k), Some(counter));
            } else {
                assert_eq!(map.get(k), None);
            }
        }

        assert!(height(&map) <= max_tree_height(n));
        map.clear();
    }

    #[test]
    pub fn test_insert_one() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, i32, i32> =
            StorageOrderedMap::new(&next_trie_id());
        assert_eq!(None, map.insert(&1, 2));
        assert_eq!(2, map.insert(&1, 3).unwrap());
    }

    #[test]
    fn test_insert_lookup_n_desc() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, i32, i32> =
            StorageOrderedMap::new(&next_trie_id());

        let n: usize = 30;
        let cases = (0..2 * (n as i32)).rev().collect::<Vec<i32>>();

        let mut counter = 0;
        for k in &cases {
            if *k % 2 == 0 {
                counter += 1;
                map.insert(k, counter);
            }
        }

        counter = 0;
        for k in &cases {
            if *k % 2 == 0 {
                counter += 1;
                assert_eq!(map.get(k), Some(counter));
            } else {
                assert_eq!(map.get(k), None);
            }
        }

        assert!(height(&map) <= max_tree_height(n));
        map.clear();
    }

    #[test]
    fn insert_n_random() {
        let _ = setup_env();

        for k in 1..5 {
            // tree size is 2^k
            let mut map: StorageOrderedMap<DebugApi, u32, u32> =
                StorageOrderedMap::new(&next_trie_id());

            let n = 1 << k;
            let input: Vec<u32> = random(n);

            for x in &input {
                map.insert(x, 42);
            }

            for x in &input {
                assert_eq!(map.get(x), Some(42));
            }

            assert!(height(&map) <= max_tree_height(n));
            map.clear();
        }
    }

    #[test]
    fn test_min() {
        let _ = setup_env();

        let n = 30;
        let vec = random(n);

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        for x in vec.iter().rev() {
            map.insert(x, 1);
        }

        assert_eq!(map.min().unwrap(), *vec.iter().min().unwrap());
        map.clear();
    }

    #[test]
    fn test_max() {
        let _ = setup_env();

        let n = 30;
        let vec = random(n);

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        for x in vec.iter().rev() {
            map.insert(x, 1);
        }

        assert_eq!(map.max().unwrap(), *vec.iter().max().unwrap());
        map.clear();
    }

    #[test]
    fn test_lower() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        let vec: Vec<u32> = vec![10, 20, 30, 40, 50];

        for x in &vec {
            map.insert(x, 1);
        }

        assert_eq!(map.lower(&5), None);
        assert_eq!(map.lower(&10), None);
        assert_eq!(map.lower(&11), Some(10));
        assert_eq!(map.lower(&20), Some(10));
        assert_eq!(map.lower(&49), Some(40));
        assert_eq!(map.lower(&50), Some(40));
        assert_eq!(map.lower(&51), Some(50));

        map.clear();
    }

    #[test]
    fn test_higher() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        let vec: Vec<u32> = vec![10, 20, 30, 40, 50];

        for x in &vec {
            map.insert(x, 1);
        }

        assert_eq!(map.higher(&5), Some(10));
        assert_eq!(map.higher(&10), Some(20));
        assert_eq!(map.higher(&11), Some(20));
        assert_eq!(map.higher(&20), Some(30));
        assert_eq!(map.higher(&49), Some(50));
        assert_eq!(map.higher(&50), None);
        assert_eq!(map.higher(&51), None);

        map.clear();
    }

    #[test]
    fn test_floor_key() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        let vec: Vec<u32> = vec![10, 20, 30, 40, 50];

        for x in &vec {
            map.insert(x, 1);
        }

        assert_eq!(map.floor_key(&5), None);
        assert_eq!(map.floor_key(&10), Some(10));
        assert_eq!(map.floor_key(&11), Some(10));
        assert_eq!(map.floor_key(&20), Some(20));
        assert_eq!(map.floor_key(&49), Some(40));
        assert_eq!(map.floor_key(&50), Some(50));
        assert_eq!(map.floor_key(&51), Some(50));

        map.clear();
    }

    #[test]
    fn test_ceil_key() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        let vec: Vec<u32> = vec![10, 20, 30, 40, 50];

        for x in &vec {
            map.insert(x, 1);
        }

        assert_eq!(map.ceil_key(&5), Some(10));
        assert_eq!(map.ceil_key(&10), Some(10));
        assert_eq!(map.ceil_key(&11), Some(20));
        assert_eq!(map.ceil_key(&20), Some(20));
        assert_eq!(map.ceil_key(&49), Some(50));
        assert_eq!(map.ceil_key(&50), Some(50));
        assert_eq!(map.ceil_key(&51), None);

        map.clear();
    }

    #[test]
    fn test_remove_1() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        map.insert(&1, 1);
        assert_eq!(map.get(&1), Some(1));
        map.remove(&1);
        assert_eq!(map.get(&1), None);
        assert_eq!(map.tree.len(), 0);
        map.clear();
    }

    #[test]
    fn test_remove_3() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = avl(&[(0, 0)], &[0, 0, 1]);

        assert!(map.iter().collect::<Vec<(u32, u32)>>().is_empty());
    }

    #[test]
    fn test_remove_3_desc() {
        let _ = setup_env();

        let vec: Vec<u32> = vec![3, 2, 1];
        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for x in &vec {
            assert_eq!(map.get(x), None);
            map.insert(x, 1);
            assert_eq!(map.get(x), Some(1));
        }

        for x in &vec {
            assert_eq!(map.get(x), Some(1));
            map.remove(x);
            assert_eq!(map.get(x), None);
        }
        map.clear();
    }

    #[test]
    fn test_remove_3_asc() {
        let _ = setup_env();

        let vec: Vec<u32> = vec![1, 2, 3];
        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for x in &vec {
            assert_eq!(map.get(x), None);
            map.insert(x, 1);
            assert_eq!(map.get(x), Some(1));
        }

        for x in &vec {
            assert_eq!(map.get(x), Some(1));
            map.remove(x);
            assert_eq!(map.get(x), None);
        }
        map.clear();
    }

    #[test]
    fn test_remove_7_regression_1() {
        let _ = setup_env();

        let vec: Vec<u32> = vec![
            2_104_297_040,
            552_624_607,
            4_269_683_389,
            3_382_615_941,
            155_419_892,
            4_102_023_417,
            1_795_725_075,
        ];
        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for x in &vec {
            assert_eq!(map.get(x), None);
            map.insert(x, 1);
            assert_eq!(map.get(x), Some(1));
        }

        for x in &vec {
            assert_eq!(map.get(x), Some(1));
            map.remove(x);
            assert_eq!(map.get(x), None);
        }
        map.clear();
    }

    #[test]
    fn test_remove_7_regression_2() {
        let _ = setup_env();

        let vec: Vec<u32> = vec![
            700_623_085,
            87_488_544,
            1_500_140_781,
            1_111_706_290,
            3_187_278_102,
            4_042_663_151,
            3_731_533_080,
        ];
        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for x in &vec {
            assert_eq!(map.get(x), None);
            map.insert(x, 1);
            assert_eq!(map.get(x), Some(1));
        }

        for x in &vec {
            assert_eq!(map.get(x), Some(1));
            map.remove(x);
            assert_eq!(map.get(x), None);
        }
        map.clear();
    }

    #[test]
    fn test_remove_9_regression() {
        let _ = setup_env();

        let vec: Vec<u32> = vec![
            1_186_903_464,
            506_371_929,
            1_738_679_820,
            1_883_936_615,
            1_815_331_350,
            1_512_669_683,
            3_581_743_264,
            1_396_738_166,
            1_902_061_760,
        ];
        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for x in &vec {
            assert_eq!(map.get(x), None);
            map.insert(x, 1);
            assert_eq!(map.get(x), Some(1));
        }

        for x in &vec {
            assert_eq!(map.get(x), Some(1));
            map.remove(x);
            assert_eq!(map.get(x), None);
        }
        map.clear();
    }

    #[test]
    fn test_remove_20_regression_1() {
        let _ = setup_env();

        let vec: Vec<u32> = vec![
            552_517_392,
            3_638_992_158,
            1_015_727_752,
            2_500_937_532,
            638_716_734,
            586_360_620,
            2_476_692_174,
            1_425_948_996,
            3_608_478_547,
            757_735_878,
            2_709_959_928,
            2_092_169_539,
            3_620_770_200,
            783_020_918,
            1_986_928_932,
            200_210_441,
            1_972_255_302,
            533_239_929,
            497_054_557,
            2_137_924_638,
        ];
        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for x in &vec {
            assert_eq!(map.get(x), None);
            map.insert(x, 1);
            assert_eq!(map.get(x), Some(1));
        }

        for x in &vec {
            assert_eq!(map.get(x), Some(1));
            map.remove(x);
            assert_eq!(map.get(x), None);
        }
        map.clear();
    }

    #[test]
    fn test_remove_7_regression() {
        let _ = setup_env();

        let vec: Vec<u32> = vec![280, 606, 163, 857, 436, 508, 44, 801];

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for x in &vec {
            assert_eq!(map.get(x), None);
            map.insert(x, 1);
            assert_eq!(map.get(x), Some(1));
        }

        for x in &vec {
            assert_eq!(map.get(x), Some(1));
            map.remove(x);
            assert_eq!(map.get(x), None);
        }

        assert_eq!(map.len(), 0, "map.len() > 0");
        assert_eq!(map.tree.len(), 0, "map.tree is not empty");
        map.clear();
    }

    #[test]
    fn test_insert_8_remove_4_regression() {
        let _ = setup_env();

        let insert = vec![882, 398, 161, 76];
        let remove = vec![242, 687, 860, 811];

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for (i, (k1, k2)) in insert.iter().zip(remove.iter()).enumerate() {
            let v = i as u32;
            map.insert(k1, v);
            map.insert(k2, v);
        }

        for k in &remove {
            map.remove(k);
        }

        assert_eq!(map.len(), insert.len());

        for (i, k) in insert.iter().enumerate() {
            assert_eq!(map.get(k), Some(i as u32));
        }
    }

    #[test]
    fn test_remove_n() {
        let _ = setup_env();

        let n = 20;
        let vec = random(n);

        let mut set: HashSet<u32> = HashSet::new();
        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        for x in &vec {
            map.insert(x, 1);
            set.insert(*x);
        }

        assert_eq!(map.len(), set.len());

        for x in &set {
            assert_eq!(map.get(x), Some(1));
            map.remove(x);
            assert_eq!(map.get(x), None);
        }

        assert_eq!(map.len(), 0, "map.len() > 0");
        assert_eq!(map.tree.len(), 0, "map.tree is not empty");
        map.clear();
    }

    #[test]
    fn test_remove_root_3() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        map.insert(&2, 1);
        map.insert(&3, 1);
        map.insert(&1, 1);
        map.insert(&4, 1);

        map.remove(&2);

        assert_eq!(map.get(&1), Some(1));
        assert_eq!(map.get(&2), None);
        assert_eq!(map.get(&3), Some(1));
        assert_eq!(map.get(&4), Some(1));
        map.clear();
    }

    #[test]
    fn test_insert_2_remove_2_regression() {
        let _ = setup_env();

        let ins: Vec<u32> = vec![11_760_225, 611_327_897];
        let rem: Vec<u32> = vec![2_982_517_385, 1_833_990_072];

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        map.insert(&ins[0], 1);
        map.insert(&ins[1], 1);

        map.remove(&rem[0]);
        map.remove(&rem[1]);

        let h = height(&map);
        let h_max = max_tree_height(map.len());
        assert!(h <= h_max, "h={h} h_max={h_max}");
        map.clear();
    }

    #[test]
    fn test_insert_n_duplicates() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        for x in 0..30 {
            map.insert(&x, x);
            map.insert(&42, x);
        }

        assert_eq!(map.get(&42), Some(29));
        assert_eq!(map.len(), 31);
        assert_eq!(map.tree.len(), 31);

        map.clear();
    }

    #[test]
    fn test_insert_2n_remove_n_random() {
        let _ = setup_env();

        for k in 1..4 {
            let mut map: StorageOrderedMap<DebugApi, u32, u32> =
                StorageOrderedMap::new(&next_trie_id());
            let mut set: HashSet<u32> = HashSet::new();

            let n = 1 << k;
            let ins: Vec<u32> = random(n);
            let rem: Vec<u32> = random(n);

            for x in &ins {
                set.insert(*x);
                map.insert(x, 42);
            }

            for x in &rem {
                set.insert(*x);
                map.insert(x, 42);
            }

            for x in &rem {
                set.remove(x);
                map.remove(x);
            }

            assert_eq!(map.len(), set.len());

            let h = height(&map);
            let h_max = max_tree_height(n);
            assert!(
                h <= h_max,
                "[n={n}] tree is too high: {h} (max is {h_max})."
            );

            map.clear();
        }
    }

    #[test]
    fn test_remove_empty() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        assert_eq!(map.remove(&1), None);
    }

    #[test]
    fn test_to_vec() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        map.insert(&1, 41);
        map.insert(&2, 42);
        map.insert(&3, 43);

        assert_eq!(map.to_vec(), vec![(1, 41), (2, 42), (3, 43)]);
        map.clear();
    }

    #[test]
    fn test_to_vec_empty() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        assert!(map.to_vec().is_empty());
    }

    #[test]
    fn test_iter() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        map.insert(&1, 41);
        map.insert(&2, 42);
        map.insert(&3, 43);

        assert_eq!(
            map.iter().collect::<Vec<(u32, u32)>>(),
            vec![(1, 41), (2, 42), (3, 43)]
        );

        // Test custom iterator impls
        assert_eq!(map.iter().nth(1), Some((2, 42)));
        assert_eq!(map.iter().count(), 3);
        assert_eq!(map.iter().last(), Some((3, 43)));
        map.clear();
    }

    #[test]
    fn test_iter_empty() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        assert_eq!(map.iter().count(), 0);
    }

    #[test]
    fn test_iter_rev() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        map.insert(&1, 41);
        map.insert(&2, 42);
        map.insert(&3, 43);

        assert_eq!(
            map.iter_rev().collect::<Vec<(u32, u32)>>(),
            vec![(3, 43), (2, 42), (1, 41)]
        );

        // Test custom iterator impls
        assert_eq!(map.iter_rev().nth(1), Some((2, 42)));
        assert_eq!(map.iter_rev().count(), 3);
        assert_eq!(map.iter_rev().last(), Some((1, 41)));
        map.clear();
    }

    #[test]
    fn test_iter_rev_empty() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        assert_eq!(map.iter_rev().count(), 0);
    }

    #[test]
    fn test_iter_from() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        let one: Vec<u32> = vec![10, 20, 30, 40, 50];
        let two: Vec<u32> = vec![45, 35, 25, 15, 5];

        for x in &one {
            map.insert(x, 42);
        }

        for x in &two {
            map.insert(x, 42);
        }

        assert_eq!(
            map.iter_from(&29).collect::<Vec<(u32, u32)>>(),
            vec![(30, 42), (35, 42), (40, 42), (45, 42), (50, 42)]
        );

        assert_eq!(
            map.iter_from(&30).collect::<Vec<(u32, u32)>>(),
            vec![(35, 42), (40, 42), (45, 42), (50, 42)]
        );

        assert_eq!(
            map.iter_from(&31).collect::<Vec<(u32, u32)>>(),
            vec![(35, 42), (40, 42), (45, 42), (50, 42)]
        );

        // Test custom iterator impls
        assert_eq!(map.iter_from(&31).nth(2), Some((45, 42)));
        assert_eq!(map.iter_from(&31).count(), 4);
        assert_eq!(map.iter_from(&31).last(), Some((50, 42)));

        map.clear();
    }

    #[test]
    fn test_iter_from_empty() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        assert_eq!(map.iter_from(&42).count(), 0);
    }

    #[test]
    fn test_iter_rev_from() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        let one: Vec<u32> = vec![10, 20, 30, 40, 50];
        let two: Vec<u32> = vec![45, 35, 25, 15, 5];

        for x in &one {
            map.insert(x, 42);
        }

        for x in &two {
            map.insert(x, 42);
        }

        assert_eq!(
            map.iter_rev_from(&29).collect::<Vec<(u32, u32)>>(),
            vec![(25, 42), (20, 42), (15, 42), (10, 42), (5, 42)]
        );

        assert_eq!(
            map.iter_rev_from(&30).collect::<Vec<(u32, u32)>>(),
            vec![(25, 42), (20, 42), (15, 42), (10, 42), (5, 42)]
        );

        assert_eq!(
            map.iter_rev_from(&31).collect::<Vec<(u32, u32)>>(),
            vec![(30, 42), (25, 42), (20, 42), (15, 42), (10, 42), (5, 42)]
        );

        // Test custom iterator impls
        assert_eq!(map.iter_rev_from(&31).nth(2), Some((20, 42)));
        assert_eq!(map.iter_rev_from(&31).count(), 6);
        assert_eq!(map.iter_rev_from(&31).last(), Some((5, 42)));

        map.clear();
    }

    #[test]
    fn test_range() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());

        let one: Vec<u32> = vec![10, 20, 30, 40, 50];
        let two: Vec<u32> = vec![45, 35, 25, 15, 5];

        for x in &one {
            map.insert(x, 42);
        }

        for x in &two {
            map.insert(x, 42);
        }

        assert_eq!(
            map.range((Bound::Included(20), Bound::Excluded(30)))
                .collect::<Vec<(u32, u32)>>(),
            vec![(20, 42), (25, 42)]
        );

        assert_eq!(
            map.range((Bound::Excluded(10), Bound::Included(40)))
                .collect::<Vec<(u32, u32)>>(),
            vec![(15, 42), (20, 42), (25, 42), (30, 42), (35, 42), (40, 42)]
        );

        assert_eq!(
            map.range((Bound::Included(20), Bound::Included(40)))
                .collect::<Vec<(u32, u32)>>(),
            vec![(20, 42), (25, 42), (30, 42), (35, 42), (40, 42)]
        );

        assert_eq!(
            map.range((Bound::Excluded(20), Bound::Excluded(45)))
                .collect::<Vec<(u32, u32)>>(),
            vec![(25, 42), (30, 42), (35, 42), (40, 42)]
        );

        assert!(map
            .range((Bound::Excluded(25), Bound::Excluded(30)))
            .collect::<Vec<(u32, u32)>>()
            .is_empty());

        assert_eq!(
            map.range((Bound::Included(25), Bound::Included(25)))
                .collect::<Vec<(u32, u32)>>(),
            vec![(25, 42)]
        );

        assert!(map
            .range((Bound::Excluded(25), Bound::Included(25)))
            .collect::<Vec<(u32, u32)>>()
            .is_empty()); // the range makes no sense, but `BTreeMap` does not panic in this case

        // Test custom iterator impls
        assert_eq!(
            map.range((Bound::Excluded(20), Bound::Excluded(45))).nth(2),
            Some((35, 42))
        );
        assert_eq!(
            map.range((Bound::Excluded(20), Bound::Excluded(45)))
                .count(),
            4
        );
        assert_eq!(
            map.range((Bound::Excluded(20), Bound::Excluded(45))).last(),
            Some((40, 42))
        );

        map.clear();
    }

    #[test]
    #[should_panic(expected = "Invalid range")]
    fn test_range_panics_same_excluded() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        let _ = map.range((Bound::Excluded(1), Bound::Excluded(1)));
    }

    #[test]
    #[should_panic(expected = "Invalid range")]
    fn test_range_panics_non_overlap_incl_exlc() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        let _ = map.range((Bound::Included(2), Bound::Excluded(1)));
    }

    #[test]
    #[should_panic(expected = "Invalid range")]
    fn test_range_panics_non_overlap_excl_incl() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        let _ = map.range((Bound::Excluded(2), Bound::Included(1)));
    }

    #[test]
    #[should_panic(expected = "Invalid range")]
    fn test_range_panics_non_overlap_incl_incl() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        let _ = map.range((Bound::Included(2), Bound::Included(1)));
    }

    #[test]
    #[should_panic(expected = "Invalid range")]
    fn test_range_panics_non_overlap_excl_excl() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        let _ = map.range((Bound::Excluded(2), Bound::Excluded(1)));
    }

    #[test]
    fn test_iter_rev_from_empty() {
        let _ = setup_env();

        let map: StorageOrderedMap<DebugApi, u32, u32> = StorageOrderedMap::new(&next_trie_id());
        assert_eq!(map.iter_rev_from(&42).count(), 0);
    }

    #[test]
    fn test_balance_regression_1() {
        let _ = setup_env();

        let insert = vec![(2, 0), (3, 0), (4, 0)];
        let remove = vec![0, 0, 0, 1];

        let map: StorageOrderedMap<DebugApi, i32, i32> = avl(&insert, &remove);
        assert!(is_balanced(&map, map.root.get()));
    }

    #[test]
    fn test_balance_regression_2() {
        let _ = setup_env();

        let insert = vec![(1, 0), (2, 0), (0, 0), (3, 0), (5, 0), (6, 0)];
        let remove = vec![0, 0, 0, 3, 5, 6, 7, 4];

        let map: StorageOrderedMap<DebugApi, i32, i32> = avl(&insert, &remove);
        assert!(is_balanced(&map, map.root.get()));
    }

    //
    // Property-based tests of AVL-based TreeMap against std::collections::BTreeMap
    //

    fn avl<SA, K, V>(insert: &[(K, V)], remove: &[K]) -> StorageOrderedMap<SA, K, V>
    where
        SA: StorageMapperApi,
        K: OrderedMapKeyTrait,
        V: OrderedMapValueTrait + Default + Copy,
    {
        let mut map: StorageOrderedMap<SA, K, V> = StorageOrderedMap::new(&next_trie_id());
        for k in remove {
            map.insert(k, Default::default());
        }
        let n = insert.len().max(remove.len());
        for i in 0..n {
            if i < remove.len() {
                map.remove(&remove[i]);
            }
            if i < insert.len() {
                let (k, v) = &insert[i];
                map.insert(k, *v);
            }
        }
        map
    }

    fn rb<K, V>(insert: &[(K, V)], remove: &[K]) -> BTreeMap<K, V>
    where
        K: Ord + Clone,
        V: Clone + Default,
    {
        let mut map: BTreeMap<K, V> = BTreeMap::default();
        for k in remove {
            map.insert(k.clone(), Default::default());
        }
        let n = insert.len().max(remove.len());
        for i in 0..n {
            if i < remove.len() {
                map.remove(&remove[i]);
            }
            if i < insert.len() {
                let (k, v) = &insert[i];
                map.insert(k.clone(), v.clone());
            }
        }
        map
    }

    #[test]
    fn prop_avl_vs_rb_simple() {
        fn prop(insert: Vec<(u32, u32)>, remove: Vec<u32>) -> bool {
            let a: StorageOrderedMap<DebugApi, u32, u32> = avl(&insert, &remove);
            let b = rb(&insert, &remove);
            let v1: Vec<(u32, u32)> = a.iter().collect();
            let v2: Vec<(u32, u32)> = b.into_iter().collect();
            v1 == v2
        }

        let _ = setup_env();

        QuickCheck::new()
            .tests(10)
            .quickcheck(prop as fn(std::vec::Vec<(u32, u32)>, std::vec::Vec<u32>) -> bool);
    }

    fn is_balanced<SA, K, V>(map: &StorageOrderedMap<SA, K, V>, root: usize) -> bool
    where
        SA: StorageMapperApi,
        K: OrderedMapKeyTrait,
        V: OrderedMapValueTrait,
    {
        let node = map.node(root).unwrap();
        let balance = map.get_balance(&node);

        (-1..=1).contains(&balance)
            && node.lft.map_or(true, |id| is_balanced(map, id))
            && node.rgt.map_or(true, |id| is_balanced(map, id))
    }

    #[test]
    fn prop_avl_balance() {
        fn prop(insert: Vec<(u32, u32)>, remove: Vec<u32>) -> bool {
            let map: StorageOrderedMap<DebugApi, u32, u32> = avl(&insert, &remove);
            map.is_empty() || is_balanced(&map, map.root.get())
        }

        let _ = setup_env();

        QuickCheck::new()
            .tests(10)
            .quickcheck(prop as fn(std::vec::Vec<(u32, u32)>, std::vec::Vec<u32>) -> bool);
    }

    #[test]
    fn prop_avl_height() {
        fn prop(insert: Vec<(u32, u32)>, remove: Vec<u32>) -> bool {
            let map: StorageOrderedMap<DebugApi, u32, u32> = avl(&insert, &remove);
            height(&map) <= max_tree_height(map.len())
        }

        let _ = setup_env();

        QuickCheck::new()
            .tests(10)
            .quickcheck(prop as fn(std::vec::Vec<(u32, u32)>, std::vec::Vec<u32>) -> bool);
    }

    fn range_prop(
        insert: Vec<(u32, u32)>,
        remove: Vec<u32>,
        range: (Bound<u32>, Bound<u32>),
    ) -> bool {
        let a: StorageOrderedMap<DebugApi, u32, u32> = avl(&insert, &remove);
        let b = rb(&insert, &remove);
        let v1: Vec<(u32, u32)> = a.range(range).collect();
        let v2: Vec<(u32, u32)> = b.range(range).map(|(k, v)| (*k, *v)).collect();
        v1 == v2
    }

    type Prop = fn(std::vec::Vec<(u32, u32)>, std::vec::Vec<u32>, u32, u32) -> bool;

    #[test]
    fn prop_avl_vs_rb_range_incl_incl() {
        fn prop(insert: Vec<(u32, u32)>, remove: Vec<u32>, r1: u32, r2: u32) -> bool {
            let range = (Bound::Included(r1.min(r2)), Bound::Included(r1.max(r2)));
            range_prop(insert, remove, range)
        }

        let _ = setup_env();

        QuickCheck::new().tests(10).quickcheck(prop as Prop);
    }

    #[test]
    fn prop_avl_vs_rb_range_incl_excl() {
        fn prop(insert: Vec<(u32, u32)>, remove: Vec<u32>, r1: u32, r2: u32) -> bool {
            let range = (Bound::Included(r1.min(r2)), Bound::Excluded(r1.max(r2)));
            range_prop(insert, remove, range)
        }

        let _ = setup_env();

        QuickCheck::new().tests(10).quickcheck(prop as Prop);
    }

    #[test]
    fn prop_avl_vs_rb_range_excl_incl() {
        fn prop(insert: Vec<(u32, u32)>, remove: Vec<u32>, r1: u32, r2: u32) -> bool {
            let range = (Bound::Excluded(r1.min(r2)), Bound::Included(r1.max(r2)));
            range_prop(insert, remove, range)
        }

        let _ = setup_env();

        QuickCheck::new().tests(10).quickcheck(prop as Prop);
    }

    #[test]
    fn prop_avl_vs_rb_range_excl_excl() {
        fn prop(insert: Vec<(u32, u32)>, remove: Vec<u32>, r1: u32, r2: u32) -> bool {
            // (Excluded(x), Excluded(x)) is invalid range, checking against it makes no sense
            r1 == r2 || {
                let range = (Bound::Excluded(r1.min(r2)), Bound::Excluded(r1.max(r2)));
                range_prop(insert, remove, range)
            }
        }

        let _ = setup_env();

        QuickCheck::new().tests(10).quickcheck(prop as Prop);
    }

    #[test]
    fn test_debug() {
        let _ = setup_env();

        let mut map: StorageOrderedMap<DebugApi, u32, u32> =
            StorageOrderedMap::new(&next_trie_id());
        map.insert(&1, 100);
        map.insert(&3, 300);
        map.insert(&2, 200);

        let node1 = "Node { id: 1, key: 1, lft: None, rgt: None, ht: 1 }";
        let node2 = "Node { id: 3, key: 2, lft: Some(1), rgt: Some(2), ht: 2 }";
        let node3 = "Node { id: 2, key: 3, lft: None, rgt: None, ht: 1 }";
        assert_eq!(
            format!("{map:?}"),
            format!("TreeMap {{ root: 3, tree: [{node1}, {node3}, {node2}] }}")
        );
    }
}
