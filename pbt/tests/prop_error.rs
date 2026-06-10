//! HTTP/2 エラー型の PBT
//!
//! RFC 9113 Section 7 および draft-ietf-webtrans-http2-14 Section 11.3 のエラーコードのプロパティをテストする
//! (WebTransport 系コードは draft では 0xTBD のため、0x100-0x102 は本実装の暫定値)。

use proptest::prelude::*;
use shiguredo_http2::{Error, ErrorCode, ErrorKind};

/// 未知のエラーコード値を生成する Strategy
fn unknown_error_code_value() -> impl Strategy<Value = u32> {
    (0u32..=u32::MAX).prop_filter(
        "must be unknown code",
        |v| !matches!(*v, 0x00..=0x0d | 0x100..=0x102),
    )
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
        Just(ErrorCode::WebtransportError),
        Just(ErrorCode::WebtransportStreamStateError),
        Just(ErrorCode::WebtransportFlowControlError),
        unknown_error_code_value().prop_map(ErrorCode::Unknown),
    ]
}

/// ErrorKind を生成する Strategy
fn error_kind_strategy() -> impl Strategy<Value = ErrorKind> {
    prop_oneof![
        error_code_strategy().prop_map(ErrorKind::ConnectionError),
        error_code_strategy().prop_map(ErrorKind::StreamError),
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
