//! エラー型

use std::fmt;
use std::io;

use shiguredo_http2::webtransport::WtError;

/// エラー型
#[derive(Debug)]
pub enum Error {
    /// I/O エラー
    Io(io::Error),
    /// HTTP/2 プロトコルエラー
    Protocol(shiguredo_http2::Error),
    /// TLS エラー
    Tls(Box<dyn std::error::Error + Send + Sync>),
    /// WebTransport セッション / Capsule 処理エラー
    WebTransport(WtError),
    /// 接続がクローズされた
    ConnectionClosed,
    /// 無効な引数
    InvalidArgument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::Protocol(e) => write!(f, "protocol error: {}", e),
            Error::Tls(e) => write!(f, "TLS error: {}", e),
            Error::WebTransport(e) => write!(f, "webtransport error: {}", e),
            Error::ConnectionClosed => write!(f, "connection closed"),
            Error::InvalidArgument(e) => write!(f, "invalid argument: {}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Protocol(e) => Some(e),
            Error::Tls(e) => Some(e.as_ref()),
            Error::WebTransport(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<shiguredo_http2::Error> for Error {
    fn from(e: shiguredo_http2::Error) -> Self {
        Error::Protocol(e)
    }
}

impl From<WtError> for Error {
    fn from(e: WtError) -> Self {
        Error::WebTransport(e)
    }
}

/// Result 型
pub type Result<T> = std::result::Result<T, Error>;
