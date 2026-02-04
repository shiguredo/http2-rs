//! shiguredo_nghttp2 - nghttp2 Rust バインディング
//!
//! nghttp2 (HTTP/2 プロトコル実装) の Rust バインディングを提供する。

mod error;
mod session;
mod types;

pub use error::{Error, Result};
pub use session::{Session, SessionRole};
pub use types::{ErrorCode, FrameType, Header, Http2Event, StreamId};

/// nghttp2 のバージョン文字列を取得
pub fn nghttp2_version() -> &'static str {
    unsafe {
        let info = nghttp2_sys::nghttp2_version(0);
        if info.is_null() {
            "unknown"
        } else {
            let version_str = (*info).version_str;
            if version_str.is_null() {
                "unknown"
            } else {
                std::ffi::CStr::from_ptr(version_str)
                    .to_str()
                    .unwrap_or("unknown")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nghttp2_version() {
        let version = nghttp2_version();
        assert!(!version.is_empty());
        assert!(version.starts_with("1."));
        println!("nghttp2 version: {}", version);
    }

    #[test]
    fn test_header_method() {
        let header = Header::method("GET");
        assert_eq!(header.name_str(), Some(":method"));
        assert_eq!(header.value_str(), Some("GET"));
    }

    #[test]
    fn test_header_status() {
        let header = Header::status(200);
        assert_eq!(header.name_str(), Some(":status"));
        assert_eq!(header.value_str(), Some("200"));
    }

    #[test]
    fn test_header_sensitive() {
        let header = Header::sensitive(b"authorization".to_vec(), b"Bearer token".to_vec());
        assert!(header.sensitive);
        assert_eq!(header.name_str(), Some("authorization"));
    }

    #[test]
    fn test_error_code() {
        assert_eq!(ErrorCode::NoError.as_u32(), 0);
        assert_eq!(ErrorCode::ProtocolError.as_u32(), 1);
        assert_eq!(ErrorCode::from_u32(0), ErrorCode::NoError);
        assert_eq!(ErrorCode::from_u32(1), ErrorCode::ProtocolError);
    }

    #[test]
    fn test_client_session_new() {
        let session = Session::client();
        assert!(session.is_ok());
        let session = session.unwrap();
        assert_eq!(session.role(), SessionRole::Client);
    }

    #[test]
    fn test_server_session_new() {
        let session = Session::server();
        assert!(session.is_ok());
        let session = session.unwrap();
        assert_eq!(session.role(), SessionRole::Server);
    }

    #[test]
    fn test_session_submit_settings() {
        let mut session = Session::client().unwrap();
        let settings = vec![
            (
                nghttp2_sys::nghttp2_settings_id_NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS as u16,
                100,
            ),
            (
                nghttp2_sys::nghttp2_settings_id_NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE as u16,
                65535,
            ),
        ];
        assert!(session.submit_settings(&settings).is_ok());
        assert!(session.want_write());
    }

    #[test]
    fn test_session_send() {
        let mut session = Session::client().unwrap();
        let settings = vec![(
            nghttp2_sys::nghttp2_settings_id_NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS as u16,
            100,
        )];
        session.submit_settings(&settings).unwrap();

        let output = session.send();
        assert!(output.is_ok());
        let output = output.unwrap();
        assert!(!output.is_empty());
        // HTTP/2 プリフェイスまたは SETTINGS フレームが含まれているはず
        println!("Output length: {} bytes", output.len());

        // 最初の 24 バイトを確認
        let preface = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
        println!("First 24 bytes: {:?}", &output[..24.min(output.len())]);
        println!("Expected preface: {:?}", preface);
        if output.starts_with(preface) {
            println!("nghttp2 includes connection preface automatically");
        } else {
            println!("nghttp2 does NOT include connection preface");
        }
    }
}
