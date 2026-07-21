//! WebTransport Exporter Context のシリアライズ
//!
//! draft-ietf-webtrans-http2-15 Section 5.3 に定義されている
//! TLS Keying Material Exporter 用のコンテキスト構造体をバイト列化する。
//!
//! 本実装が参照する仕様は IETF draft (`-15`) であり、draft の改訂や RFC 化に
//! 伴って章番号・要求項目が変わりうる。

use crate::webtransport::error::WtError;

/// WebTransport Exporter Context をシリアライズする
///
/// draft-ietf-webtrans-http2-15 Section 5.3:
/// ```text
/// WebTransport Exporter Context {
///   WebTransport Session ID (64),
///   WebTransport Application-Supplied Exporter Label Length (8),
///   WebTransport Application-Supplied Exporter Label (8..),
///   WebTransport Application-Supplied Exporter Context Length (8),
///   WebTransport Application-Supplied Exporter Context (..)
/// }
/// ```
///
/// `app_label` / `app_context` はそれぞれ最大 255 バイト。超過時は
/// `WtError::invalid_input` を返す。
pub fn serialize_exporter_context(
    session_id: u64,
    app_label: &[u8],
    app_context: &[u8],
) -> Result<Vec<u8>, WtError> {
    if app_label.len() > 255 {
        return Err(WtError::invalid_input("exporter label exceeds 255 bytes"));
    }
    if app_context.len() > 255 {
        return Err(WtError::invalid_input("exporter context exceeds 255 bytes"));
    }

    // 入力由来サイズでの事前割当は行わない (shiguredo-rust 規約)
    let mut out = Vec::new();
    out.extend_from_slice(&session_id.to_be_bytes());
    // 255 以下であることを上で確認済みのため u8 へのキャストは安全
    out.push(app_label.len() as u8);
    out.extend_from_slice(app_label);
    out.push(app_context.len() as u8);
    out.extend_from_slice(app_context);
    Ok(out)
}
