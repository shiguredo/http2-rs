use shiguredo_http2::decode_error::DecodeError;
use shiguredo_http2::error::{Error, ErrorCode, ErrorKind};

/// RFC 9113 Section 7 で定義された既知のエラーコード値
const KNOWN_ERROR_CODES: &[(u32, ErrorCode)] = &[
    (0x00, ErrorCode::NoError),
    (0x01, ErrorCode::ProtocolError),
    (0x02, ErrorCode::InternalError),
    (0x03, ErrorCode::FlowControlError),
    (0x04, ErrorCode::SettingsTimeout),
    (0x05, ErrorCode::StreamClosed),
    (0x06, ErrorCode::FrameSizeError),
    (0x07, ErrorCode::RefusedStream),
    (0x08, ErrorCode::Cancel),
    (0x09, ErrorCode::CompressionError),
    (0x0a, ErrorCode::ConnectError),
    (0x0b, ErrorCode::EnhanceYourCalm),
    (0x0c, ErrorCode::InadequateSecurity),
    (0x0d, ErrorCode::Http11Required),
    (0x100, ErrorCode::WebtransportError),
    (0x101, ErrorCode::WebtransportStreamStateError),
    (0x102, ErrorCode::WebtransportFlowControlError),
];

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

/// RFC 9113 Section 7 で定義された既知のエラーコードのマッピングをテスト
#[test]
fn test_known_error_codes_mapping() {
    for (value, expected_code) in KNOWN_ERROR_CODES {
        let code = ErrorCode::from_u32(*value);
        assert_eq!(
            code, *expected_code,
            "値 0x{:x} は {:?} にマップされるべき",
            value, expected_code
        );
        assert_eq!(
            code.as_u32(),
            *value,
            "{:?} は 0x{:x} に逆変換されるべき",
            code,
            value
        );
    }
}

/// 0x0e から 0xff の範囲は未知のエラーコード
#[test]
fn test_gap_values_are_unknown() {
    for value in 0x0e..0x100 {
        let code = ErrorCode::from_u32(value);
        assert!(
            matches!(code, ErrorCode::Unknown(v) if v == value),
            "値 0x{:x} は Unknown であるべき",
            value
        );
    }
    // 0x103 以降も未知
    for value in 0x103..0x110 {
        let code = ErrorCode::from_u32(value);
        assert!(
            matches!(code, ErrorCode::Unknown(v) if v == value),
            "値 0x{:x} は Unknown であるべき",
            value
        );
    }
}

/// Display 出力にファイルパスが含まれないこと
#[test]
fn test_display_excludes_location() {
    let err = Error::connection_error(ErrorCode::ProtocolError, "test reason");
    let display = format!("{err}");
    assert!(
        !display.contains('/'),
        "Display にファイルパスが含まれていないこと: {display}"
    );
    assert!(
        !display.contains('\\'),
        "Display にファイルパスが含まれていないこと: {display}"
    );
    assert!(
        display.contains("PROTOCOL_ERROR"),
        "Display にエラー種別が含まれていること"
    );
    assert!(
        display.contains("test reason"),
        "Display に理由が含まれていること"
    );
}

/// Display 出力に "Backtrace" が含まれないこと
#[test]
fn test_display_excludes_backtrace() {
    let err = Error::connection_error(ErrorCode::InternalError, "");
    let display = format!("{err}");
    assert!(
        !display.contains("Backtrace"),
        "Display に Backtrace が含まれていないこと"
    );
}

/// Debug 通常フォーマットにバックトレースが含まれないこと
#[test]
fn test_debug_excludes_backtrace() {
    let err = Error::connection_error(ErrorCode::FlowControlError, "debug test");
    let debug = format!("{err:?}");
    assert!(
        !debug.contains("Backtrace"),
        "Debug に Backtrace が含まれていないこと"
    );
    assert!(
        debug.contains('/'),
        "Debug にファイルパスが含まれていること"
    );
}

/// Debug alternate format にバックトレース出力用の分岐が存在すること
#[test]
fn test_debug_alternate_accepts_backtrace() {
    let err = Error::connection_error(ErrorCode::Cancel, "alt debug");
    let alt_debug = format!("{err:#?}");
    assert!(
        alt_debug.contains('/'),
        "Debug alternate にファイルパスが含まれていること"
    );
}
