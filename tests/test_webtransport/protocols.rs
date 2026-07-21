//! `WtAvailableProtocols` / `serialize_wt_protocol` の単体テスト
//!
//! draft-ietf-webtrans-http3 の Application Protocol Negotiation と
//! RFC 8941 List of String / sf-string の境界を検証する。

use shiguredo_http2::webtransport::{WtAvailableProtocols, WtErrorKind, serialize_wt_protocol};

/// 正常系: 複数 String が preference order で保持されること
#[test]
fn test_parse_multiple_strings() {
    let parsed = WtAvailableProtocols::parse(br#""echo", "raw""#)
        .expect(r#""echo", "raw" はパースできるはず"#);
    assert_eq!(
        parsed.protocols,
        vec!["echo".to_string(), "raw".to_string()],
        "preference order (= 入力順) が保持されること"
    );
}

/// 単一エントリ
#[test]
fn test_parse_single_string() {
    let parsed = WtAvailableProtocols::parse(br#""echo""#).expect(r#""echo" はパースできるはず"#);
    assert_eq!(parsed.protocols, vec!["echo".to_string()]);
}

/// 空入力は空 List として成功すること
#[test]
fn test_parse_empty_input() {
    let parsed = WtAvailableProtocols::parse(b"").expect("空入力は空 List として成功するはず");
    assert!(
        parsed.protocols.is_empty(),
        "空入力の protocols は空であること"
    );
}

/// Token は String 以外として拒否されること
#[test]
fn test_parse_rejects_token() {
    let err = WtAvailableProtocols::parse(b"echo").expect_err("Token は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");
}

/// Integer は String 以外として拒否されること
#[test]
fn test_parse_rejects_integer() {
    let err = WtAvailableProtocols::parse(b"123").expect_err("Integer は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");
}

/// Boolean は String 以外として拒否されること
#[test]
fn test_parse_rejects_boolean() {
    let err = WtAvailableProtocols::parse(b"?1").expect_err("Boolean は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");
}

/// Byte Sequence は String 以外として拒否されること
#[test]
fn test_parse_rejects_byte_sequence() {
    let err = WtAvailableProtocols::parse(b":YWJj:").expect_err("Byte Sequence は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");
}

/// パラメータは無視され、String 値だけが残ること
#[test]
fn test_parse_ignores_parameters() {
    let parsed = WtAvailableProtocols::parse(br#""echo";version=1, "raw";v=2"#)
        .expect("パラメータ付き List はパースできるはず");
    assert_eq!(
        parsed.protocols,
        vec!["echo".to_string(), "raw".to_string()],
        "パラメータは値の取得を阻害しないこと"
    );
}

/// `\"` / `\\` エスケープが復元されること
#[test]
fn test_parse_escape_sequences() {
    let parsed = WtAvailableProtocols::parse(br#""hello\"world""#)
        .expect(r#"エスケープ付き String はパースできるはず"#);
    assert_eq!(
        parsed.protocols,
        vec!["hello\"world".to_string()],
        r#"\" が " に復元されること"#
    );

    let parsed =
        WtAvailableProtocols::parse(br#""a\\b""#).expect(r#"\\ エスケープはパースできるはず"#);
    assert_eq!(
        parsed.protocols,
        vec!["a\\b".to_string()],
        r#"\\ が \ に復元されること"#
    );
}

/// 重複エントリは List セマンティクスどおり保持されること
#[test]
fn test_parse_allows_duplicates() {
    let parsed = WtAvailableProtocols::parse(br#""a", "a", "b""#)
        .expect("重複を含む List はパースできるはず");
    assert_eq!(
        parsed.protocols,
        vec!["a".to_string(), "a".to_string(), "b".to_string()]
    );
}

/// 非 ASCII (0x80+) は拒否されること
#[test]
fn test_parse_rejects_non_ascii() {
    let err = WtAvailableProtocols::parse(b"\"\x80\"").expect_err("非 ASCII は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");
}

/// trailing comma は拒否されること
#[test]
fn test_parse_rejects_trailing_comma() {
    let err =
        WtAvailableProtocols::parse(br#""echo","#).expect_err("trailing comma は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");
}

/// 無効なエスケープ列は拒否されること
#[test]
fn test_parse_rejects_invalid_escape() {
    let err = WtAvailableProtocols::parse(br#""a\nb""#)
        .expect_err(r#"\n のような無効エスケープは拒否されるはず"#);
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");
}

/// serialize: 通常値は DQUOTE で囲まれること
#[test]
fn test_serialize_basic() {
    let out = serialize_wt_protocol(b"echo").expect("通常値はシリアライズできるはず");
    assert_eq!(out, b"\"echo\"");
}

/// serialize: `"` と `\` がエスケープされること
#[test]
fn test_serialize_escapes_quote_and_backslash() {
    let out = serialize_wt_protocol(br#"a"b"#).expect(r#" " を含む値はシリアライズできるはず"#);
    assert_eq!(out, br#""a\"b""#);

    let out = serialize_wt_protocol(br#"a\b"#).expect(r#"\ を含む値はシリアライズできるはず"#);
    assert_eq!(out, br#""a\\b""#);
}

/// serialize: ASCII printable 外は拒否されること
#[test]
fn test_serialize_rejects_non_printable() {
    let err = serialize_wt_protocol(b"a\nb").expect_err("0x20 未満は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");

    let err = serialize_wt_protocol(b"a\x7fb").expect_err("0x7F 以上は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");

    let err = serialize_wt_protocol(b"\x80").expect_err("非 ASCII は拒否されるはず");
    assert_eq!(err.kind, WtErrorKind::InvalidInput, "実際: {err}");
}
