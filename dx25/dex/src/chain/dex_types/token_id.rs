use std::{fmt::Debug, hash::Hash};

use multiversx_sc::{
    api::ManagedTypeApi,
    derive::TypeAbi,
    types::{ManagedBuffer, TokenIdentifier},
};
use multiversx_sc_codec::{
    self as codec,
    derive::{NestedDecode, NestedEncode, TopDecode, TopEncode},
};

#[derive(Clone, TopDecode, TopEncode, NestedDecode, NestedEncode, TypeAbi)]
pub struct TokenId<M: ManagedTypeApi>(TokenIdentifier<M>);

impl<M: ManagedTypeApi> TokenId<M> {
    pub fn new(token_id: TokenIdentifier<M>) -> Self {
        Self(token_id)
    }

    pub fn from_bytes<B: Into<ManagedBuffer<M>>>(bytes: B) -> Self {
        Self(TokenIdentifier::from_esdt_bytes(bytes.into()))
    }

    pub fn native(&self) -> &TokenIdentifier<M> {
        &self.0
    }
}

impl<M: ManagedTypeApi> Debug for TokenId<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Token: {:?}", self.0)
    }
}

impl<M: ManagedTypeApi> PartialEq for TokenId<M> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<M: ManagedTypeApi> Eq for TokenId<M> {}

impl<M: ManagedTypeApi> PartialOrd for TokenId<M> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // The handle actually contains a handle to a memory slice, so we need to compare actual slices
        self.0
            .to_boxed_bytes()
            .as_ref()
            .partial_cmp(other.0.to_boxed_bytes().as_ref())
    }
}

impl<M: ManagedTypeApi> Ord for TokenId<M> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // The handle actually contains a handle to a memory slice, so we need to compare actual slices
        self.0
            .to_boxed_bytes()
            .as_ref()
            .cmp(other.0.to_boxed_bytes().as_ref())
    }
}

impl<M: ManagedTypeApi> Hash for TokenId<M> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_boxed_bytes().as_ref().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use multiversx_sc_scenario::DebugApi;

    use crate::chain::TokenId;

    #[test]
    fn test_token_id_tree() {
        let _ = DebugApi::dummy();

        let mut map: BTreeMap<TokenId, u32> = BTreeMap::new();
        let token1 = TokenId::from_bytes("0");
        let token2 = TokenId::from_bytes("1");
        let token3 = TokenId::from_bytes("2");

        map.insert(token1.clone(), 1);
        map.insert(token2.clone(), 2);

        assert_eq!(map.get(&token1), Some(&1));
        assert_eq!(map.get(&token2), Some(&2));
        assert_eq!(map.get(&token3), None);

        map.insert(token3.clone(), 3);

        assert_eq!(map.get(&token1), Some(&1));
        assert_eq!(map.get(&token2), Some(&2));
        assert_eq!(map.get(&token3), Some(&3));
    }
}
