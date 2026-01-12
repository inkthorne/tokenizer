use thiserror::Error;

#[derive(Error, Debug)]
pub enum TokenizerError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Invalid index format: {0}")]
    InvalidIndexFormat(String),

    #[error("Directory walk error: {0}")]
    WalkDir(String),

    #[error("Index not found: {0}")]
    IndexNotFound(String),
}

impl From<std::io::Error> for TokenizerError {
    fn from(err: std::io::Error) -> Self {
        TokenizerError::Io(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, TokenizerError>;
