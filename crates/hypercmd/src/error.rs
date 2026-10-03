use std::{convert::Infallible, fmt};

/// The boundary at which preparation or interaction failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Construction,
    Template,
    DuplicateKey,
    ReentrantEvent,
    Publication,
    Navigation,
    Service,
    Disposed,
    Limit,
    Terminal,
}

/// A recoverable update leaves its committed subtree unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn limit_if(condition: bool, message: &str) -> Result<(), Self> {
        (!condition)
            .then_some(())
            .ok_or_else(|| Self::limit(message))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for Error {}

impl From<Infallible> for Error {
    fn from(error: Infallible) -> Self {
        match error {}
    }
}
impl From<fusor::ContextError> for Error {
    fn from(error: fusor::ContextError) -> Self {
        Self::template(error.to_string())
    }
}
impl From<taffy::TaffyError> for Error {
    fn from(error: taffy::TaffyError) -> Self {
        Self::template(error.to_string())
    }
}

macro_rules! kinds {
    ($($method:ident => $kind:ident),* $(,)?) => {
        impl Error { $(pub(crate) fn $method(message: impl Into<String>) -> Self {
            Self::new(ErrorKind::$kind, message)
        })* }
    };
}
kinds!(template => Template, limit => Limit, service => Service, navigation => Navigation, disposed => Disposed);
#[cfg(feature = "native")]
kinds!(terminal => Terminal);

/// Event handlers may return either `()` or a recoverable error.
pub trait EventResult {
    fn into_result(self) -> Result<(), Error>;
}
impl EventResult for () {
    fn into_result(self) -> Result<(), Error> {
        Ok(())
    }
}
impl EventResult for Result<(), Error> {
    fn into_result(self) -> Result<(), Error> {
        self
    }
}
