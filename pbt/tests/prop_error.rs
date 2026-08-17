//! HTTP/2 エラー型の PBT
//!
//! RFC 9113 Section 7 および draft-ietf-webtrans-http2-15 Section 11.3 のエラーコードのプロパティをテストする
//! (WebTransport 系コードは draft では 0xTBD のため、0x100-0x102 は本実装の暫定値)。

use shiguredo_http2::{Error, ErrorCode, ErrorKind};

/// 各 PBT 共通のシード取得用環境変数名
///
/// 失敗時に表示される hex シードをそのまま代入して再現する。
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// 未知のエラーコード値を生成する
///
/// 既知コード (0x00..=0x0d, 0x100..=0x102) を除外した全域からの一様サンプリング。
/// 許容率がほぼ 1 のため `sample_with_rejection` の試行回数は 8 で十分。
fn sample_unknown_error_code_value(ctx: &mut noprop::TestCaseContext) -> u32 {
    let is_known = |v: u32| matches!(v, 0x00..=0x0d | 0x100..=0x102);
    noprop::sample_with_rejection(ctx, 8, |ctx| {
        let v = noprop::sample_u32(ctx);
        (!is_known(v)).then_some(v)
    })
}

/// ErrorCode を生成する
fn sample_error_code(ctx: &mut noprop::TestCaseContext) -> ErrorCode {
    match noprop::sample_weighted_index(ctx, &[1; 17]) {
        0 => ErrorCode::NoError,
        1 => ErrorCode::ProtocolError,
        2 => ErrorCode::InternalError,
        3 => ErrorCode::FlowControlError,
        4 => ErrorCode::SettingsTimeout,
        5 => ErrorCode::StreamClosed,
        6 => ErrorCode::FrameSizeError,
        7 => ErrorCode::RefusedStream,
        8 => ErrorCode::Cancel,
        9 => ErrorCode::CompressionError,
        10 => ErrorCode::ConnectError,
        11 => ErrorCode::EnhanceYourCalm,
        12 => ErrorCode::InadequateSecurity,
        13 => ErrorCode::Http11Required,
        14 => ErrorCode::WtError,
        15 => ErrorCode::WtStreamStateError,
        16 => ErrorCode::WtFlowControlError,
        _ => unreachable!("sample_weighted_index は 0..17 を返す"),
    }
}

/// ErrorKind を生成する
fn sample_error_kind(ctx: &mut noprop::TestCaseContext) -> ErrorKind {
    match noprop::sample_weighted_index(ctx, &[1, 1, 1]) {
        0 => ErrorKind::ConnectionError(sample_error_code(ctx)),
        1 => ErrorKind::StreamError(sample_error_code(ctx)),
        _ => ErrorKind::HpackError,
    }
}

/// 小文字 ASCII (a-z) の文字列を生成する
fn sample_lowercase_string(ctx: &mut noprop::TestCaseContext, len: usize) -> String {
    (0..len)
        .map(|_| (b'a' + noprop::sample_usize_in(ctx, 0..26) as u8) as char)
        .collect()
}

/// ErrorCode のラウンドトリップ: from_u32(code.as_u32()) == code
///
/// RFC 9113 Section 7: エラーコードは情報を失わずに変換できる
#[test]
fn prop_error_code_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let code = sample_error_code(ctx);
        let value = code.as_u32();
        let roundtripped = ErrorCode::from_u32(value);
        assert_eq!(roundtripped, code);
        Ok(())
    })?;
    Ok(())
}

/// 未知のエラーコード値の保持
///
/// RFC 9113 Section 7: 未知のエラーコードは Unknown として保持され、
/// 生の値は失われない
#[test]
fn prop_unknown_error_code_preserves_value() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = sample_unknown_error_code_value(ctx);
        let code = ErrorCode::from_u32(value);
        assert_eq!(code, ErrorCode::Unknown(value));
        assert_eq!(code.as_u32(), value);
        Ok(())
    })?;
    Ok(())
}

/// 任意の u32 値のラウンドトリップ
///
/// すべての u32 値に対して as_u32(from_u32(x)) == x
#[test]
fn prop_any_u32_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = noprop::sample_u32(ctx);
        let code = ErrorCode::from_u32(value);
        assert_eq!(code.as_u32(), value);
        Ok(())
    })?;
    Ok(())
}

/// Unknown エラーコードの Display が値を含む
#[test]
fn prop_unknown_error_code_display_contains_value() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = sample_unknown_error_code_value(ctx);
        let code = ErrorCode::Unknown(value);
        let display = format!("{}", code);
        assert!(
            display.contains("UNKNOWN"),
            "Display は UNKNOWN を含む必要がある: {display}"
        );
        // 16 進数表記で値を含む
        let hex_value = format!("{value:x}");
        assert!(
            display.contains(&hex_value),
            "Display は 16 進数表記の値 {value:#x} を含む必要がある: {display}"
        );
        Ok(())
    })?;
    Ok(())
}

/// Error の is_connection_error と is_stream_error の相互排他性
#[test]
fn prop_error_type_mutual_exclusivity() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let kind = sample_error_kind(ctx);
        let error = Error::new(kind);

        match kind {
            ErrorKind::ConnectionError(_) => {
                assert!(error.is_connection_error());
                assert!(!error.is_stream_error());
                assert!(error.error_code().is_some());
            }
            ErrorKind::StreamError(_) => {
                assert!(!error.is_connection_error());
                assert!(error.is_stream_error());
                assert!(error.error_code().is_some());
            }
            _ => {
                assert!(!error.is_connection_error());
                assert!(!error.is_stream_error());
                assert!(error.error_code().is_none());
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// Error の error_code() が正しい ErrorCode を返す
#[test]
fn prop_error_code_extraction() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let code = sample_error_code(ctx);
        let conn_error = Error::connection_error(code, "test");
        let stream_error = Error::stream_error(code, "test");

        assert_eq!(conn_error.error_code(), Some(code));
        assert_eq!(stream_error.error_code(), Some(code));
        Ok(())
    })?;
    Ok(())
}

/// Error の Display が kind を含む
#[test]
fn prop_error_display_contains_kind() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let kind = sample_error_kind(ctx);
        let error = Error::new(kind);
        let display = format!("{}", error);
        let kind_str = format!("{}", kind);

        // Display に kind の表示が含まれる
        assert!(
            display.contains(&kind_str),
            "Display は kind の表示 {kind_str:?} を含む必要がある: {display}"
        );
        Ok(())
    })?;
    Ok(())
}

/// Error の reason が Display に反映される
#[test]
fn prop_error_display_contains_reason() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let kind = sample_error_kind(ctx);
        let len = noprop::sample_usize_in(ctx, 1..=20);
        let reason = sample_lowercase_string(ctx, len);
        let error = Error::with_reason(kind, reason.clone());
        let display = format!("{}", error);

        assert!(
            display.contains(&reason),
            "Display は reason {reason:?} を含む必要がある: {display}"
        );
        Ok(())
    })?;
    Ok(())
}
