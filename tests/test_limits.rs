//! `Limits` のデフォルト値境界テスト。

use shiguredo_http2::{Limits, LimitsError};

/// デフォルト値で `Limits::builder().build()` は常に成功する。
///
/// `Limits::builder()` 経由でビルダーを生成し、何も設定せずに build した場合に
/// 成功することを保証する境界テスト (デフォルト値が常に妥当である不変条件)。
#[test]
fn test_default_build_succeeds() {
    let result = Limits::builder().build();
    assert!(result.is_ok());
}

/// `wt_enabled=false` + WT 初期設定ありで `build()` が失敗する。
///
/// draft-ietf-webtrans-http2-15 Section 3.1: WT 初期設定 (SETTINGS_WT_INITIAL_MAX_*) は
/// SETTINGS_WT_ENABLED=1 のサポート表明があって意味を持つ。
#[test]
fn test_wt_initial_settings_without_wt_enabled_fails() {
    let result = Limits::builder()
        .enable_connect_protocol(true)
        .wt_enabled(false)
        .webtransport(Some(1024), None, None, None, None, None)
        .build();
    assert_eq!(
        result.expect_err("should fail"),
        LimitsError::WebtransportRequiresWtEnabled
    );
}

/// `wt_enabled=true` + `enable_connect_protocol=false` で `build()` が失敗する。
///
/// draft-ietf-webtrans-http2-15 Section 3.1: WebTransport を構成するなら
/// SETTINGS_ENABLE_CONNECT_PROTOCOL=1 が必須。
#[test]
fn test_wt_enabled_without_connect_protocol_fails() {
    let result = Limits::builder()
        .enable_connect_protocol(false)
        .wt_enabled(true)
        .build();
    assert_eq!(
        result.expect_err("should fail"),
        LimitsError::WebtransportRequiresConnectProtocol
    );
}

/// `wt_enabled=true` + `enable_connect_protocol=true` + WT 初期設定ありで `build()` が成功する。
#[test]
fn test_wt_enabled_with_connect_protocol_and_initial_settings_succeeds() {
    let result = Limits::builder()
        .enable_connect_protocol(true)
        .wt_enabled(true)
        .webtransport(
            Some(1024),
            Some(256),
            Some(256),
            Some(10),
            Some(10),
            Some(256),
        )
        .build();
    assert!(result.is_ok());
    let limits = result.expect("should succeed");
    assert!(limits.wt_enabled());
    assert_eq!(limits.wt_initial_max_data(), Some(1024));
}
