//! nghttp2 エラー型

use std::ffi::CStr;
use std::fmt;

/// nghttp2 のエラー型
#[derive(Debug)]
pub enum Error {
    /// nghttp2 エラー
    Nghttp2(String, i32),

    /// 無効な引数
    InvalidArgument(String),

    /// バッファ不足
    BufferTooSmall,

    /// ストリームが見つからない
    StreamNotFound(i32),

    /// セッションが閉じている
    SessionClosed,

    /// コールバックエラー
    Callback(String),

    /// 内部エラー
    Internal(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Nghttp2(msg, code) => write!(f, "nghttp2 error: {} ({})", msg, code),
            Error::InvalidArgument(msg) => write!(f, "invalid argument: {}", msg),
            Error::BufferTooSmall => write!(f, "buffer too small"),
            Error::StreamNotFound(id) => write!(f, "stream not found: {}", id),
            Error::SessionClosed => write!(f, "session is closed"),
            Error::Callback(msg) => write!(f, "callback error: {}", msg),
            Error::Internal(msg) => write!(f, "internal error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

/// Result 型エイリアス
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// nghttp2 エラーコードからエラーを生成
    pub fn from_nghttp2(code: libc::c_int) -> Self {
        let msg = unsafe {
            let ptr = nghttp2_sys::nghttp2_strerror(code);
            if ptr.is_null() {
                "unknown error".to_string()
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        };
        Error::Nghttp2(msg, code)
    }
}

/// nghttp2 の結果をチェック
pub fn check_nghttp2(code: libc::c_int) -> Result<()> {
    if code < 0 {
        Err(Error::from_nghttp2(code))
    } else {
        Ok(())
    }
}

/// nghttp2 の結果をチェック（戻り値を返す）
pub fn check_nghttp2_with_value(code: libc::c_int) -> Result<libc::c_int> {
    if code < 0 {
        Err(Error::from_nghttp2(code))
    } else {
        Ok(code)
    }
}
