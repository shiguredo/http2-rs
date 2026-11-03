//! `Settings` のデフォルト値・SETTINGS 識別子・`to_settings_list` の契約テスト。

use shiguredo_http2::settings::{
    DEFAULT_ENABLE_PUSH, DEFAULT_HEADER_TABLE_SIZE, DEFAULT_INITIAL_WINDOW_SIZE,
    DEFAULT_MAX_FRAME_SIZE,
};
use shiguredo_http2::{Setting, Settings};

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
    // draft-ietf-webtrans-http2-16 Section 3.1: デフォルト値は 0 (非サポート)
    assert!(!settings.wt_enabled());
}

/// SETTINGS_WT_ENABLED (0x2b67) の wire 値検証。
///
/// draft-ietf-webtrans-http2-16 Section 3.1: 値は 0 または 1 のみ。
/// 1 より大きい値は接続エラー PROTOCOL_ERROR。
#[test]
fn test_wt_enabled_from_wire_rejects_greater_than_one() {
    // 0 は有効 (非サポート)
    let setting = Setting::from_wire(0x2b67, 0);
    assert_eq!(
        setting.expect("有効な SETTINGS でなければならない"),
        Setting::WtEnabled(false)
    );

    // 1 は有効 (サポート)
    let setting = Setting::from_wire(0x2b67, 1);
    assert_eq!(
        setting.expect("有効な SETTINGS でなければならない"),
        Setting::WtEnabled(true)
    );

    // 2 は無効
    let setting = Setting::from_wire(0x2b67, 2);
    assert_eq!(
        setting.expect_err("拒否されなければならない"),
        shiguredo_http2::SettingError::WtEnabledNotBoolean { value: 2 }
    );

    // u32::MAX も無効
    let setting = Setting::from_wire(0x2b67, u32::MAX);
    assert_eq!(
        setting.expect_err("拒否されなければならない"),
        shiguredo_http2::SettingError::WtEnabledNotBoolean { value: u32::MAX }
    );
}

/// SETTINGS_WT_ENABLED の SETTINGS 識別子の検証。
///
/// draft-ietf-webtrans-http2-16 Section 11.2: SETTINGS_WT_ENABLED の識別子は 0x2b67。
/// draft-ietf-webtrans-http2-15 の 0x2b60 は未知の SETTINGS として扱い、有効化にも値域違反にもしない
/// (draft-ietf-webtrans-http2-16 Section 9.1: 非互換な変更では新しい識別子を使う)。
#[test]
fn test_wt_enabled_wire_id() {
    assert_eq!(Setting::WtEnabled(false).as_wire(), (0x2b67, 0));
    assert_eq!(Setting::WtEnabled(true).as_wire(), (0x2b67, 1));

    // 旧識別子は未知の SETTINGS として無視する (RFC 9113 Section 6.5.2)
    let setting = Setting::from_wire(0x2b60, 1);
    assert_eq!(
        setting.expect("未知の SETTINGS は無視されなければならない"),
        Setting::Unknown {
            id: 0x2b60,
            value: 1,
        }
    );

    // 旧識別子の値 2 も boolean の値域違反として拒否しない
    let setting = Setting::from_wire(0x2b60, 2);
    assert_eq!(
        setting.expect("未知の SETTINGS は無視されなければならない"),
        Setting::Unknown {
            id: 0x2b60,
            value: 2,
        }
    );

    // 未知の SETTINGS は保持した識別子のまま wire へ戻る
    // (識別子を正規化すると未知の識別子の送受信検証が成立しなくなる)
    assert_eq!(
        Setting::Unknown {
            id: 0x2b60,
            value: 7,
        }
        .as_wire(),
        (0x2b60, 7)
    );
}

/// `to_settings_list` の出力規則と SETTINGS 識別子の昇順の契約を検証する。
///
/// - 値が `None` の項目は出力しない
/// - boolean フラグは `EnablePush` を除き true のときだけ出力する
/// - 並びは SETTINGS 識別子の昇順でなければならない
#[test]
fn test_to_settings_list_order_and_omission() {
    // 既定値: None と false のフラグが省略され、必須項目のみが残る
    let minimal = Settings::default().to_settings_list();
    let ids: Vec<u16> = minimal.iter().map(|setting| setting.as_wire().0).collect();
    assert_eq!(ids, vec![0x01, 0x02, 0x04, 0x05]);

    // 全項目を設定した場合: SETTINGS 識別子の昇順で過不足なく出力される
    let mut settings = Settings::default();
    settings.apply(Setting::MaxConcurrentStreams(100));
    settings.apply(Setting::MaxHeaderListSize(4096));
    settings.apply(Setting::EnableConnectProtocol(true));
    settings.apply(Setting::NoRfc7540Priorities(true));
    settings.apply(Setting::WtInitialMaxData(1));
    settings.apply(Setting::WtInitialMaxStreamDataUni(2));
    settings.apply(Setting::WtInitialMaxStreamDataBidiLocal(3));
    settings.apply(Setting::WtInitialMaxStreamsUni(4));
    settings.apply(Setting::WtInitialMaxStreamsBidi(5));
    settings.apply(Setting::WtInitialMaxStreamDataBidiRemote(6));
    settings.apply(Setting::WtEnabled(true));
    // 適用した値が getter から取得できること (公開 API の動作保証)
    assert_eq!(settings.wt_initial_max_data(), Some(1));
    assert_eq!(settings.wt_initial_max_stream_data_uni(), Some(2));
    assert_eq!(settings.wt_initial_max_stream_data_bidi_local(), Some(3));
    assert_eq!(settings.wt_initial_max_streams_uni(), Some(4));
    assert_eq!(settings.wt_initial_max_streams_bidi(), Some(5));
    assert_eq!(settings.wt_initial_max_stream_data_bidi_remote(), Some(6));
    assert!(settings.wt_enabled());
    let ids: Vec<u16> = settings
        .to_settings_list()
        .iter()
        .map(|setting| setting.as_wire().0)
        .collect();
    assert_eq!(
        ids,
        vec![
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x08, 0x09, 0x2b61, 0x2b62, 0x2b63, 0x2b64, 0x2b65,
            0x2b66, 0x2b67,
        ]
    );
}
