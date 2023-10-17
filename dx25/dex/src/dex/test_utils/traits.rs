/// Serialization adapter for different blockchains
pub trait TestSer {
    /// Serialize value into specified writer
    fn ser(&self, writer: &mut impl std::io::Write);
    /// Serialize value into byte vector
    fn to_vec(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.ser(&mut buf);
        buf
    }
}
/// Deserialization adapter for different blockchains
pub trait TestDe: Sized {
    /// Deserialize value from provided byte slice
    fn de(buf: &mut &[u8]) -> Self;
}

#[cfg(feature = "near")]
mod near {
    use super::{TestDe, TestSer};
    use std::io;

    pub type PersistentBound = ();

    impl<T: near_sdk::borsh::BorshSerialize> TestSer for T {
        fn ser(&self, writer: &mut impl io::Write) {
            self.serialize(writer).unwrap();
        }
    }

    impl<T: Sized + near_sdk::borsh::BorshDeserialize> TestDe for T {
        fn de(buf: &mut &[u8]) -> Self {
            Self::deserialize(buf).unwrap()
        }
    }
}

#[cfg(feature = "near")]
pub use near::PersistentBound;

#[cfg(feature = "concordium")]
mod concordium {
    use super::{TestDe, TestSer};
    use concordium_std::{
        DeserialWithState, HasStateApi, HasStateEntry, ParseResult, Read, Seek, Write,
    };
    use std::io;
    use thiserror::Error;

    pub type PersistentBound = StateApiStub;

    #[derive(Debug, Error, Default)]
    pub enum WriteError {
        #[default]
        #[error("Unspecified write error")]
        Default,
        #[error("{0}")]
        Io(#[from] io::Error),
    }

    struct Writer<'a, W: io::Write>(&'a mut W);

    impl<'a, W: io::Write> concordium_std::Write for Writer<'a, W> {
        type Err = WriteError;

        fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, Self::Err> {
            self.0.write(buf).map_err(Into::into)
        }
    }

    impl<T: concordium_std::Serial> TestSer for T {
        fn ser(&self, writer: &mut impl io::Write) {
            self.serial(&mut Writer(writer)).unwrap();
        }
    }

    struct Reader<'a, 'b>(&'a mut &'b [u8]);

    impl<'a, 'b> concordium_std::Read for Reader<'a, 'b> {
        fn read(&mut self, buf: &mut [u8]) -> concordium_std::ParseResult<usize> {
            let len = buf.len().min(self.0.len());
            buf[..len].copy_from_slice(&self.0[..len]);
            *self.0 = &self.0[len..];
            Ok(len)
        }
    }

    impl<T: DeserialWithState<StateApiStub>> TestDe for T {
        fn de(buf: &mut &[u8]) -> Self
        where
            Self: Sized,
        {
            Self::deserial_with_state(&StateApiStub, &mut Reader(buf)).unwrap()
        }
    }
    /// Stub type for `HasStateApi` implementations
    ///
    /// Not actually used during deserialization process,
    /// since all Concordium-specific containers are replaced with Map.
    /// We could in principle go for proper state api implementation,
    /// but that's not feasible, as we need all this only for test harness.
    #[derive(Default, Clone)]
    pub struct StateApiStub;

    impl HasStateApi for StateApiStub {
        type EntryType = Self;

        type IterType = Self;

        fn create_entry(
            &mut self,
            _key: &[u8],
        ) -> Result<Self::EntryType, concordium_std::StateError> {
            unimplemented!()
        }

        fn lookup_entry(&self, _key: &[u8]) -> Option<Self::EntryType> {
            unimplemented!()
        }

        fn delete_entry(
            &mut self,
            _key: Self::EntryType,
        ) -> Result<(), concordium_std::StateError> {
            unimplemented!()
        }

        fn delete_prefix(&mut self, _prefix: &[u8]) -> Result<bool, concordium_std::StateError> {
            unimplemented!()
        }

        fn iterator(&self, _prefix: &[u8]) -> Result<Self::IterType, concordium_std::StateError> {
            unimplemented!()
        }

        fn delete_iterator(&mut self, _iter: Self::IterType) {
            unimplemented!()
        }
    }

    impl Iterator for StateApiStub {
        type Item = Self;

        fn next(&mut self) -> Option<Self::Item> {
            unimplemented!()
        }
    }

    impl Read for StateApiStub {
        fn read(&mut self, _buf: &mut [u8]) -> ParseResult<usize> {
            unimplemented!()
        }
    }

    impl Write for StateApiStub {
        type Err = Self;

        fn write(&mut self, _buf: &[u8]) -> Result<usize, Self::Err> {
            unimplemented!()
        }
    }

    impl Seek for StateApiStub {
        type Err = Self;

        fn seek(&mut self, _pos: concordium_std::SeekFrom) -> Result<u32, Self::Err> {
            unimplemented!()
        }

        fn cursor_position(&self) -> u32 {
            unimplemented!()
        }
    }

    impl HasStateEntry for StateApiStub {
        type StateEntryData = Self;

        type StateEntryKey = Self;

        type Error = Self;

        fn move_to_start(&mut self) {
            unimplemented!()
        }

        fn size(&self) -> Result<u32, Self::Error> {
            unimplemented!()
        }

        fn truncate(&mut self, _new_size: u32) -> Result<(), Self::Error> {
            unimplemented!()
        }

        fn get_key(&self) -> &[u8] {
            unimplemented!()
        }

        fn resize(&mut self, _new_size: u32) -> Result<(), Self::Error> {
            unimplemented!()
        }
    }
}

#[cfg(feature = "concordium")]
pub use concordium::PersistentBound;

/// MultiversX-specific bridges and proxies
#[cfg(feature = "multiversx")]
mod multiversx {
    use super::{TestDe, TestSer};
    use multiversx_sc::codec::{NestedDecode, NestedEncode};
    use std::io;

    pub type PersistentBound = ();

    impl<T: NestedEncode> TestSer for T {
        fn ser(&self, writer: &mut impl io::Write) {
            let mut out_vec: Vec<u8> = vec![];
            self.dep_encode(&mut out_vec).unwrap();

            writer.write_all(&out_vec).unwrap();
        }
    }

    impl<T: Sized + NestedDecode> TestDe for T {
        fn de(buf: &mut &[u8]) -> Self {
            Self::dep_decode(buf).unwrap()
        }
    }
}

#[cfg(feature = "multiversx")]
pub use multiversx::PersistentBound;
