//! HTTP/2 エラー型の PBT
//!
//! RFC 9113 Section 7 で定義されるエラーコードのプロパティをテストする。

use proptest::prelude::*;
use shiguredo_http2::{Error, ErrorCode, ErrorKind};

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
    (0x100, ErrorCode::WebtransportSessionGone),
];

/// 既知のエラーコード値を生成する Strategy
fn known_error_code_value() -> impl Strategy<Value = u32> {
    prop_oneof![
        Just(0x00),
        Just(0x01),
        Just(0x02),
        Just(0x03),
        Just(0x04),
        Just(0x05),
        Just(0x06),
        Just(0x07),
        Just(0x08),
        Just(0x09),
        Just(0x0a),
        Just(0x0b),
        Just(0x0c),
        Just(0x0d),
        Just(0x100),
    ]
}

/// 未知のエラーコード値を生成する Strategy
fn unknown_error_code_value() -> impl Strategy<Value = u32> {
    (0u32..=u32::MAX).prop_filter("must be unknown code", |v| {
        !matches!(*v, 0x00..=0x0d | 0x100)
    })
}

/// ErrorCode を生成する Strategy
fn error_code_strategy() -> impl Strategy<Value = ErrorCode> {
    prop_oneof![
        Just(ErrorCode::NoError),
        Just(ErrorCode::ProtocolError),
        Just(ErrorCode::InternalError),
        Just(ErrorCode::FlowControlError),
        Just(ErrorCode::SettingsTimeout),
        Just(ErrorCode::StreamClosed),
        Just(ErrorCode::FrameSizeError),
        Just(ErrorCode::RefusedStream),
        Just(ErrorCode::Cancel),
        Just(ErrorCode::CompressionError),
        Just(ErrorCode::ConnectError),
        Just(ErrorCode::EnhanceYourCalm),
        Just(ErrorCode::InadequateSecurity),
        Just(ErrorCode::Http11Required),
        Just(ErrorCode::WebtransportSessionGone),
        unknown_error_code_value().prop_map(ErrorCode::Unknown),
    ]
}

/// ErrorKind を生成する Strategy
fn error_kind_strategy() -> impl Strategy<Value = ErrorKind> {
    prop_oneof![
        error_code_strategy().prop_map(ErrorKind::ConnectionError),
        error_code_strategy().prop_map(ErrorKind::StreamError),
        Just(ErrorKind::BufferTooShort),
        Just(ErrorKind::Incomplete),
        Just(ErrorKind::InvalidInput),
        Just(ErrorKind::HpackError),
    ]
}

proptest! {
    /// ErrorCode のラウンドトリップ: from_u32(code.as_u32()) == code
    ///
    /// RFC 9113 Section 7: エラーコードは情報を失わずに変換できる
    #[test]
    fn prop_error_code_roundtrip(code in error_code_strategy()) {
        let value = code.as_u32();
        let roundtripped = ErrorCode::from_u32(value);
        prop_assert_eq!(roundtripped, code);
    }

    /// 既知のエラーコード値のラウンドトリップ
    ///
    /// RFC 9113 Section 7: 既知のエラーコードは正しい variant に変換される
    #[test]
    fn prop_known_error_code_from_u32(value in known_error_code_value()) {
        let code = ErrorCode::from_u32(value);
        // Unknown にならないことを確認
        prop_assert!(!matches!(code, ErrorCode::Unknown(_)));
        // ラウンドトリップ
        prop_assert_eq!(code.as_u32(), value);
    }

    /// 未知のエラーコード値の保持
    ///
    /// RFC 9113 Section 7: 未知のエラーコードは Unknown として保持され、
    /// 生の値は失われない
    #[test]
    fn prop_unknown_error_code_preserves_value(value in unknown_error_code_value()) {
        let code = ErrorCode::from_u32(value);
        prop_assert_eq!(code, ErrorCode::Unknown(value));
        prop_assert_eq!(code.as_u32(), value);
    }

    /// 任意の u32 値のラウンドトリップ
    ///
    /// すべての u32 値に対して as_u32(from_u32(x)) == x
    #[test]
    fn prop_any_u32_roundtrip(value in any::<u32>()) {
        let code = ErrorCode::from_u32(value);
        prop_assert_eq!(code.as_u32(), value);
    }

    /// ErrorCode の Display 実装が空でない
    #[test]
    fn prop_error_code_display_not_empty(code in error_code_strategy()) {
        let display = format!("{}", code);
        prop_assert!(!display.is_empty());
    }

    /// Unknown エラーコードの Display が値を含む
    #[test]
    fn prop_unknown_error_code_display_contains_value(value in unknown_error_code_value()) {
        let code = ErrorCode::Unknown(value);
        let display = format!("{}", code);
        prop_assert!(display.contains("UNKNOWN"));
        // 16 進数表記で値を含む
        let hex_value = format!("{:x}", value);
        prop_assert!(display.contains(&hex_value));
    }

    /// ErrorKind の Display 実装が空でない
    #[test]
    fn prop_error_kind_display_not_empty(kind in error_kind_strategy()) {
        let display = format!("{}", kind);
        prop_assert!(!display.is_empty());
    }

    /// ConnectionError と StreamError の ErrorKind は ErrorCode を含む
    #[test]
    fn prop_error_kind_with_code_display_contains_code(code in error_code_strategy()) {
        let conn_kind = ErrorKind::ConnectionError(code);
        let stream_kind = ErrorKind::StreamError(code);

        let conn_display = format!("{}", conn_kind);
        let stream_display = format!("{}", stream_kind);

        // ConnectionError/StreamError という文字列を含む
        prop_assert!(conn_display.contains("ConnectionError"));
        prop_assert!(stream_display.contains("StreamError"));
    }

    /// Error の is_connection_error と is_stream_error の相互排他性
    #[test]
    fn prop_error_type_mutual_exclusivity(kind in error_kind_strategy()) {
        let error = Error::new(kind);

        match kind {
            ErrorKind::ConnectionError(_) => {
                prop_assert!(error.is_connection_error());
                prop_assert!(!error.is_stream_error());
                prop_assert!(error.error_code().is_some());
            }
            ErrorKind::StreamError(_) => {
                prop_assert!(!error.is_connection_error());
                prop_assert!(error.is_stream_error());
                prop_assert!(error.error_code().is_some());
            }
            _ => {
                prop_assert!(!error.is_connection_error());
                prop_assert!(!error.is_stream_error());
                prop_assert!(error.error_code().is_none());
            }
        }
    }

    /// Error の error_code() が正しい ErrorCode を返す
    #[test]
    fn prop_error_code_extraction(code in error_code_strategy()) {
        let conn_error = Error::connection_error(code, "test");
        let stream_error = Error::stream_error(code, "test");

        prop_assert_eq!(conn_error.error_code(), Some(code));
        prop_assert_eq!(stream_error.error_code(), Some(code));
    }

    /// Error の Display が kind を含む
    #[test]
    fn prop_error_display_contains_kind(kind in error_kind_strategy()) {
        let error = Error::new(kind);
        let display = format!("{}", error);
        let kind_str = format!("{}", kind);

        // Display に kind の表示が含まれる
        prop_assert!(display.contains(&kind_str));
    }

    /// Error の reason が Display に反映される
    #[test]
    fn prop_error_display_contains_reason(
        kind in error_kind_strategy(),
        reason in "[a-z]{1,20}",
    ) {
        let error = Error::with_reason(kind, reason.clone());
        let display = format!("{}", error);

        prop_assert!(display.contains(&reason));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9113 Section 7 で定義された既知のエラーコードのマッピングをテスト
    #[test]
    fn test_known_error_codes_mapping() {
        for (value, expected_code) in KNOWN_ERROR_CODES {
            let code = ErrorCode::from_u32(*value);
            assert_eq!(
                code, *expected_code,
                "value 0x{:x} should map to {:?}",
                value, expected_code
            );
            assert_eq!(
                code.as_u32(),
                *value,
                "{:?} should convert back to 0x{:x}",
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
                "value 0x{:x} should be Unknown",
                value
            );
        }
    }
}
