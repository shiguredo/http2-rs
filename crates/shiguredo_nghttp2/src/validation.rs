//! HTTP/2 検証・ユーティリティ関数

use crate::types::ErrorCode;

/// ヘッダー名が RFC 9113 に準拠しているか検証
pub fn check_header_name(name: &[u8]) -> bool {
    unsafe { nghttp2_sys::nghttp2_check_header_name(name.as_ptr(), name.len()) != 0 }
}

/// ヘッダー値が RFC 9113 に準拠しているか検証
pub fn check_header_value_rfc9113(value: &[u8]) -> bool {
    unsafe { nghttp2_sys::nghttp2_check_header_value_rfc9113(value.as_ptr(), value.len()) != 0 }
}

/// HTTP メソッドが有効か検証
pub fn check_method(method: &[u8]) -> bool {
    unsafe { nghttp2_sys::nghttp2_check_method(method.as_ptr(), method.len()) != 0 }
}

/// パスが有効か検証
pub fn check_path(path: &[u8]) -> bool {
    unsafe { nghttp2_sys::nghttp2_check_path(path.as_ptr(), path.len()) != 0 }
}

/// authority が有効か検証
pub fn check_authority(authority: &[u8]) -> bool {
    unsafe { nghttp2_sys::nghttp2_check_authority(authority.as_ptr(), authority.len()) != 0 }
}

/// HTTP/2 エラーコードの説明文字列を取得
pub fn http2_strerror(error_code: ErrorCode) -> &'static str {
    unsafe {
        let ptr = nghttp2_sys::nghttp2_http2_strerror(error_code.as_u32());
        if ptr.is_null() {
            "unknown error"
        } else {
            std::ffi::CStr::from_ptr(ptr)
                .to_str()
                .unwrap_or("unknown error")
        }
    }
}

/// nghttp2 ライブラリエラーコードが致命的かどうかを判定
pub fn is_fatal(lib_error_code: i32) -> bool {
    unsafe { nghttp2_sys::nghttp2_is_fatal(lib_error_code) != 0 }
}
