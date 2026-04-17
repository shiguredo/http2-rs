//! wt_server のエラー型

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Tokio(tokio_http2::Error),
    Tls(String),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O: {e}"),
            Error::Tokio(e) => write!(f, "tokio-http2: {e}"),
            Error::Tls(e) => write!(f, "TLS: {e}"),
            Error::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<tokio_http2::Error> for Error {
    fn from(e: tokio_http2::Error) -> Self {
        Error::Tokio(e)
    }
}
