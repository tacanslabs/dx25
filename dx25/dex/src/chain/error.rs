pub const WEGLD_NOT_INIT_ERROR: &str = "wEGld integration not initialized";
pub const WEGLD_DOUBLE_INIT_ERROR: &str = "wEGld integration already initialized";

/// Stub error type. We never use it, but always call `sc_panic!`
pub type Error = usize;

/// Stub implementation for error descriminants. We never use it, but always call `sc_panic!`
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ErrorDiscriminants {
    Error,
}

impl ErrorDiscriminants {
    pub fn from_repr(_: usize) -> Option<Self> {
        Some(Self::Error)
    }
}

impl From<&ErrorDiscriminants> for &str {
    fn from(_: &ErrorDiscriminants) -> Self {
        "Custom internal error"
    }
}

impl From<&Error> for ErrorDiscriminants {
    fn from(_: &Error) -> Self {
        Self::Error
    }
}
