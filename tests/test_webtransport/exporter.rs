//! `serialize_exporter_context` の単体テスト
//!
//! draft-ietf-webtrans-http2-15 Section 5.3 の WebTransport Exporter Context
//! シリアライズ境界を検証する。

use shiguredo_http2::webtransport::{WtErrorKind, serialize_exporter_context};

/// 空の label / context でも正しいレイアウトになること
#[test]
fn test_serialize_empty_label_and_context() {
    let out = serialize_exporter_context(0x0102_0304_0506_0708, b"", b"")
        .expect("空 label / context はシリアライズできるはず");
    // session_id (8) + label_len (1) + context_len (1) = 10
    assert_eq!(out.len(), 10, "出力サイズが期待値と一致すること");
    assert_eq!(
        &out[0..8],
        &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
        "session_id は big-endian 8 バイトであること"
    );
    assert_eq!(out[8], 0, "空 label の長さは 0");
    assert_eq!(out[9], 0, "空 context の長さは 0");
}

/// 通常の label / context が長さ付きで書き込まれること
#[test]
fn test_serialize_with_label_and_context() {
    let out = serialize_exporter_context(42, b"label", b"ctx")
        .expect("通常の label / context はシリアライズできるはず");
    assert_eq!(
        out.len(),
        8 + 1 + 5 + 1 + 3,
        "出力サイズが 8+1+label+1+context であること"
    );
    assert_eq!(&out[0..8], &42u64.to_be_bytes());
    assert_eq!(out[8], 5);
    assert_eq!(&out[9..14], b"label");
    assert_eq!(out[14], 3);
    assert_eq!(&out[15..18], b"ctx");
}

/// label / context がそれぞれ 255 バイトちょうどなら成功すること
#[test]
fn test_serialize_max_length_255() {
    let label = vec![b'a'; 255];
    let context = vec![b'b'; 255];
    let out = serialize_exporter_context(1, &label, &context)
        .expect("255 バイト境界は受け入れられるはず");
    assert_eq!(out.len(), 8 + 1 + 255 + 1 + 255);
    assert_eq!(out[8], 255);
    assert_eq!(out[8 + 1 + 255], 255);
}

/// label が 256 バイトなら invalid_input になること
#[test]
fn test_serialize_rejects_label_over_255() {
    let label = vec![0u8; 256];
    let err = serialize_exporter_context(0, &label, b"")
        .expect_err("256 バイトの label は拒否されるはず");
    assert_eq!(
        err.kind(),
        WtErrorKind::InvalidInput,
        "エラー種別は InvalidInput であること、実際: {err}"
    );
}

/// context が 256 バイトなら invalid_input になること
#[test]
fn test_serialize_rejects_context_over_255() {
    let context = vec![0u8; 256];
    let err = serialize_exporter_context(0, b"", &context)
        .expect_err("256 バイトの context は拒否されるはず");
    assert_eq!(
        err.kind(),
        WtErrorKind::InvalidInput,
        "エラー種別は InvalidInput であること、実際: {err}"
    );
}

/// 異なる session_id は異なるバイト列になること
#[test]
fn test_serialize_different_session_id_differs() {
    let a = serialize_exporter_context(1, b"x", b"y").expect("シリアライズ成功");
    let b = serialize_exporter_context(2, b"x", b"y").expect("シリアライズ成功");
    assert_ne!(
        a, b,
        "session_id が異なれば出力も異なること (exporter 分離の根拠)"
    );
}
