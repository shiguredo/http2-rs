//! エラー型

use std::fmt;
use std::io;

/// エラー型
#[derive(Debug)]
pub enum Error {
    /// I/O エラー
    Io(io::Error),
    /// nghttp2 エラー
    Nghttp2(shiguredo_nghttp2::Error),
    /// TLS エラー
    Tls(Box<dyn std::error::Error + Send + Sync>),
    /// 接続がクローズされた
    ConnectionClosed,
    /// タイムアウト
    Timeout,
    /// 無効な引数
    InvalidArgument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::Nghttp2(e) => write!(f, "nghttp2 error: {}", e),
            Error::Tls(e) => write!(f, "TLS error: {}", e),
            Error::ConnectionClosed => write!(f, "connection closed"),
            Error::Timeout => write!(f, "timeout"),
            Error::InvalidArgument(e) => write!(f, "invalid argument: {}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Nghttp2(e) => Some(e),
            Error::Tls(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<shiguredo_nghttp2::Error> for Error {
    fn from(e: shiguredo_nghttp2::Error) -> Self {
        Error::Nghttp2(e)
    }
}

/// Result 型
pub type Result<T> = std::result::Result<T, Error>;
