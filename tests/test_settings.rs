//! `Settings` のデフォルト値境界テスト。

use shiguredo_http2::Settings;
use shiguredo_http2::settings::{
    DEFAULT_ENABLE_PUSH, DEFAULT_HEADER_TABLE_SIZE, DEFAULT_INITIAL_WINDOW_SIZE,
    DEFAULT_MAX_FRAME_SIZE,
};

/// デフォルト値の検証。
///
/// RFC 9113 Section 6.5.2 のデフォルト値 (ただし ENABLE_PUSH は RFC 初期値 1 に対し、
/// 本実装はサーバープッシュ非サポートのため独自デフォルト 0 を採用)。
/// 設定可能なフィールド (`max_concurrent_streams` / `max_header_list_size` / `enable_connect_protocol` /
/// `no_rfc7540_priorities` / `wt_enabled`) が `Settings::default()` 時点で「未設定相当」(`None` または `false`) であることも確認する。
#[test]
fn test_default_values() {
    let settings = Settings::default();

    assert_eq!(settings.header_table_size(), DEFAULT_HEADER_TABLE_SIZE);
    assert_eq!(settings.enable_push(), DEFAULT_ENABLE_PUSH);
    assert_eq!(settings.max_concurrent_streams(), None);
    assert_eq!(
        settings.initial_window_size().get(),
        DEFAULT_INITIAL_WINDOW_SIZE
    );
    assert_eq!(settings.max_frame_size().get(), DEFAULT_MAX_FRAME_SIZE);
    assert_eq!(settings.max_header_list_size(), None);
    assert!(!settings.enable_connect_protocol());
    assert!(!settings.no_rfc7540_priorities());
    // draft-ietf-webtrans-http2-15 Section 3.1: デフォルト値は 0 (非サポート)
    assert!(!settings.wt_enabled());
}

/// SETTINGS_WT_ENABLED (0x2b60) の wire 値検証。
///
/// draft-ietf-webtrans-http2-15 Section 3.1: 値は 0 または 1 のみ。
/// 1 より大きい値は接続エラー PROTOCOL_ERROR。
#[test]
fn test_wt_enabled_from_wire_rejects_greater_than_one() {
    // 0 は有効 (非サポート)
    let setting = shiguredo_http2::Setting::from_wire(0x2b60, 0);
    assert!(setting.is_ok());
    assert_eq!(
        setting.expect("valid setting"),
        shiguredo_http2::Setting::WtEnabled(false)
    );

    // 1 は有効 (サポート)
    let setting = shiguredo_http2::Setting::from_wire(0x2b60, 1);
    assert!(setting.is_ok());
    assert_eq!(
        setting.expect("valid setting"),
        shiguredo_http2::Setting::WtEnabled(true)
    );

    // 2 は無効
    let setting = shiguredo_http2::Setting::from_wire(0x2b60, 2);
    assert!(setting.is_err());
    assert_eq!(
        setting.expect_err("should be rejected"),
        shiguredo_http2::SettingError::WtEnabledNotBoolean { value: 2 }
    );

    // u32::MAX も無効
    let setting = shiguredo_http2::Setting::from_wire(0x2b60, u32::MAX);
    assert!(setting.is_err());
}
