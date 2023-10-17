use std::borrow::Borrow;
use std::collections::btree_map::Range;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::ops::{Bound, Deref, RangeBounds};
use std::rc::Rc;

type Bytes = Vec<u8>;
type RcBytes = Rc<Bytes>;

/// Wraps storage key, to make it comparable with anything which resembles byte slice
#[derive(Clone, Default, Hash, PartialOrd, Ord, PartialEq, Eq)]
pub struct Key(RcBytes);
/// Manual implementation of `Debug` ensures pretty-printing won't be used for
/// inner vector, and we won't get line-per-byte in output
impl Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Key: {:?}", &**self))
    }
}

impl Deref for Key {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0[..]
    }
}

impl Key {
    fn from_bytes(bytes: &impl Borrow<[u8]>) -> Self {
        Self(Rc::new(bytes.borrow().to_vec()))
    }
}

impl Borrow<Vec<u8>> for Key {
    fn borrow(&self) -> &Vec<u8> {
        &self.0
    }
}

impl Borrow<[u8]> for Key {
    fn borrow(&self) -> &[u8] {
        self
    }
}
/// Value kept in storage
#[derive(Clone)]
pub struct Value(RcBytes);
/// Manual implementation of `Debug` ensures pretty-printing won't be used for
/// inner vector, and we won't get line-per-byte in output
impl Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Value: {:?}", &**self)
    }
}

impl Deref for Value {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl Value {
    fn new() -> Self {
        Self(Rc::new(Vec::new()))
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn mutate(&mut self) -> &mut Vec<u8> {
        Rc::make_mut(&mut self.0)
    }
}

/// Mutable key-value storage, where both keys and values are just byte vectors
#[derive(Default)]
pub struct Storage(BTreeMap<Key, Value>);

/// Generate range end key for specified prefix, to match all keys which start with it
/// `None` means range must be unbounded on right end
fn prefix_end(prefix: &[u8]) -> Option<Bytes> {
    let mut prefix = prefix.to_vec();
    let mut overflow = true;
    // Increase all prefix bytes by 1 until there's no overflow
    for b in prefix.iter_mut().rev() {
        (*b, overflow) = b.overflowing_add(1);
        if !overflow {
            break;
        }
    }
    if overflow {
        None
    } else {
        Some(prefix)
    }
}

#[allow(unused)]
impl Storage {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    /// Create snapshot out of mutable storage
    ///
    /// Owned values will be re-shared
    pub fn freeze(&mut self) -> Snapshot {
        Snapshot(self.0.clone())
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn contains(&self, key: impl Borrow<[u8]>) -> bool {
        self.0.contains_key(key.borrow())
    }

    pub fn get(&self, key: impl Borrow<[u8]>) -> Option<&Value> {
        self.0.get(key.borrow())
    }

    pub fn get_mut(&mut self, key: impl Borrow<[u8]>) -> Option<&mut Value> {
        self.0.get_mut(key.borrow())
    }

    pub fn remove(&mut self, key: impl Borrow<[u8]>) -> Option<Value> {
        self.0.remove(key.borrow())
    }

    pub fn get_or_insert(&mut self, key: impl Borrow<[u8]>) -> &mut Value {
        self.0
            .entry(Key::from_bytes(&key))
            .or_insert_with(Value::new)
    }
    /// Get pair of bounds which correspond to keys with specified prefix
    pub fn prefix_bounds(prefix: impl Borrow<[u8]>) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
        let begin = prefix.borrow().to_owned();
        if let Some(end) = prefix_end(&begin) {
            (Bound::Included(begin), Bound::Excluded(end))
        } else {
            (Bound::Included(begin), Bound::Unbounded)
        }
    }
    /// Get range of entries described by specified range bounds
    pub fn range<T: ?Sized + Ord>(&self, range: impl RangeBounds<T>) -> EntryRange<'_>
    where
        Key: Borrow<T>,
    {
        self.0.range(range)
    }

    pub fn prefix_range(&self, prefix: impl Borrow<[u8]>) -> EntryRange<'_> {
        self.range(Self::prefix_bounds(prefix))
    }
    /// Retain only entries which match specified predicate
    pub fn retain(&mut self, mut pred: impl FnMut(&Key, &mut Value) -> bool) {
        self.0.retain(pred);
    }
    /// Retains only values whose keys don't start with specified prefix
    pub fn remove_prefix(&mut self, prefix: impl Borrow<[u8]>) {
        let prefix = prefix.borrow();
        self.retain(|k, _| !k.starts_with(prefix));
    }
}

pub type EntryRange<'a> = Range<'a, Key, Value>;

/// Immutable state snapshot
#[derive(Default, Clone)]
pub struct Snapshot(BTreeMap<Key, Value>);

#[allow(unused)]
impl Snapshot {
    /// Construct new empty snapshot
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    /// Create new mutable storage out of current snapshot
    pub fn thaw(&self) -> Storage {
        Storage(self.0.clone())
    }
}
