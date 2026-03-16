/// Errors returned by tool operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
