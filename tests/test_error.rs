use shiguredo_http2::decode_error::DecodeError;
use shiguredo_http2::error::{Error, ErrorCode, ErrorKind};

#[test]
fn from_decode_error_buffer_too_short() {
    let decode_err = DecodeError::BufferTooShort {
        required: 9,
        available: 4,
    };
    let err: Error = decode_err.into();
    assert!(err.is_connection_error());
    assert_eq!(err.error_code(), Some(ErrorCode::FrameSizeError));
    assert!(err.reason.contains("9"));
    assert!(err.reason.contains("4"));
}

#[test]
fn from_decode_error_incomplete() {
    let decode_err = DecodeError::Incomplete;
    let err: Error = decode_err.into();
    assert!(err.is_connection_error());
    assert_eq!(err.error_code(), Some(ErrorCode::FrameSizeError));
    assert!(err.reason.contains("incomplete"));
}

#[test]
fn error_kind_display_connection_error() {
    let kind = ErrorKind::ConnectionError(ErrorCode::ProtocolError);
    assert_eq!(kind.to_string(), "ConnectionError(PROTOCOL_ERROR)");
}

#[test]
fn error_kind_display_stream_error() {
    let kind = ErrorKind::StreamError(ErrorCode::Cancel);
    assert_eq!(kind.to_string(), "StreamError(CANCEL)");
}

#[test]
fn error_kind_display_hpack_error() {
    let kind = ErrorKind::HpackError;
    assert_eq!(kind.to_string(), "HpackError");
}
