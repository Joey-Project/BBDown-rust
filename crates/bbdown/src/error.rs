use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("{input_kind} links require an explicit selection in non-interactive mode")]
    SelectionRequired { input_kind: &'static str },
    #[error("unsupported input for this release slice: {0}")]
    Unsupported(String),
    #[error("API returned code {code}: {message}")]
    Api { code: i64, message: String },
    #[error("access restricted: {0}")]
    AccessRestricted(String),
    #[error("API response did not contain {0}")]
    MissingField(&'static str),
    #[error("failed to parse URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
