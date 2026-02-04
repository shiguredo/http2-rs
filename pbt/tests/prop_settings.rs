//! SETTINGS パラメータの PBT (RFC 9113 Section 6.5.2)
//!
//! HTTP/2 SETTINGS パラメータの検証を行う。

use proptest::prelude::*;
use shiguredo_http2::settings::{
    DEFAULT_ENABLE_PUSH, DEFAULT_HEADER_TABLE_SIZE, DEFAULT_INITIAL_WINDOW_SIZE,
    DEFAULT_MAX_FRAME_SIZE, MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, MIN_MAX_FRAME_SIZE,
};
use shiguredo_http2::{Setting, SettingId, Settings};

/// 有効な SETTINGS を生成する
fn valid_setting() -> impl Strategy<Value = Setting> {
    prop_oneof![
        // HEADER_TABLE_SIZE: 任意の u32 値が有効
        any::<u32>().prop_map(|v| Setting::from_setting_id(SettingId::HeaderTableSize, v)),
        // ENABLE_PUSH: 0 または 1 のみ有効
        prop::bool::ANY.prop_map(|b| Setting::from_setting_id(SettingId::EnablePush, u32::from(b))),
        // MAX_CONCURRENT_STREAMS: 任意の u32 値が有効
        any::<u32>().prop_map(|v| Setting::from_setting_id(SettingId::MaxConcurrentStreams, v)),
        // INITIAL_WINDOW_SIZE: 0..=2^31-1 が有効
        (0..=MAX_INITIAL_WINDOW_SIZE)
            .prop_map(|v| Setting::from_setting_id(SettingId::InitialWindowSize, v)),
        // MAX_FRAME_SIZE: 16384..=16777215 が有効
        (MIN_MAX_FRAME_SIZE..=MAX_MAX_FRAME_SIZE)
            .prop_map(|v| Setting::from_setting_id(SettingId::MaxFrameSize, v)),
        // MAX_HEADER_LIST_SIZE: 任意の u32 値が有効
        any::<u32>().prop_map(|v| Setting::from_setting_id(SettingId::MaxHeaderListSize, v)),
        // ENABLE_CONNECT_PROTOCOL: 0 または 1 のみ有効 (RFC 8441)
        prop::bool::ANY
            .prop_map(|b| Setting::from_setting_id(SettingId::EnableConnectProtocol, u32::from(b))),
        // NO_RFC7540_PRIORITIES: 0 または 1 のみ有効
        prop::bool::ANY
            .prop_map(|b| Setting::from_setting_id(SettingId::NoRfc7540Priorities, u32::from(b))),
    ]
}

/// 無効な ENABLE_PUSH 値を生成 (2 以上)
fn invalid_enable_push() -> impl Strategy<Value = Setting> {
    (2..=u32::MAX).prop_map(|v| Setting::from_setting_id(SettingId::EnablePush, v))
}

/// 無効な INITIAL_WINDOW_SIZE 値を生成 (2^31 以上)
fn invalid_initial_window_size() -> impl Strategy<Value = Setting> {
    ((MAX_INITIAL_WINDOW_SIZE + 1)..=u32::MAX)
        .prop_map(|v| Setting::from_setting_id(SettingId::InitialWindowSize, v))
}

/// 無効な MAX_FRAME_SIZE 値を生成 (範囲外)
fn invalid_max_frame_size() -> impl Strategy<Value = Setting> {
    prop_oneof![
        // MIN_MAX_FRAME_SIZE 未満
        (0..MIN_MAX_FRAME_SIZE).prop_map(|v| Setting::from_setting_id(SettingId::MaxFrameSize, v)),
        // MAX_MAX_FRAME_SIZE 超過
        ((MAX_MAX_FRAME_SIZE + 1)..=u32::MAX)
            .prop_map(|v| Setting::from_setting_id(SettingId::MaxFrameSize, v)),
    ]
}

/// 無効な ENABLE_CONNECT_PROTOCOL 値を生成 (2 以上)
fn invalid_enable_connect_protocol() -> impl Strategy<Value = Setting> {
    (2..=u32::MAX).prop_map(|v| Setting::from_setting_id(SettingId::EnableConnectProtocol, v))
}

/// 無効な NO_RFC7540_PRIORITIES 値を生成 (2 以上)
fn invalid_no_rfc7540_priorities() -> impl Strategy<Value = Setting> {
    (2..=u32::MAX).prop_map(|v| Setting::from_setting_id(SettingId::NoRfc7540Priorities, v))
}

/// 未知の SETTINGS ID を生成
fn unknown_setting() -> impl Strategy<Value = Setting> {
    // 既知の ID (0x01-0x06, 0x08, 0x09) を避ける
    (0x0a..=0xffffu16, any::<u32>()).prop_map(|(id, value)| Setting::new(id, value))
}

proptest! {
    /// 有効な SETTINGS は常に受け入れられる
    ///
    /// 数学的意義: 有効入力の受理
    #[test]
    fn prop_valid_settings_accepted(
        setting in valid_setting(),
    ) {
        let mut settings = Settings::default();
        let result = settings.apply(setting);
        prop_assert!(
            result.is_ok(),
            "Valid setting {:?} should be accepted, but got {:?}",
            setting,
            result
        );
    }

    /// 無効な ENABLE_PUSH は拒否される
    ///
    /// 数学的意義: 二値性の検証 (0 または 1 のみ)
    #[test]
    fn prop_invalid_enable_push_rejected(
        setting in invalid_enable_push(),
    ) {
        let mut settings = Settings::default();
        let result = settings.apply(setting);
        prop_assert!(
            result.is_err(),
            "Invalid ENABLE_PUSH {:?} should be rejected",
            setting
        );
    }

    /// 無効な INITIAL_WINDOW_SIZE は拒否される
    ///
    /// 数学的意義: 範囲制約 (0..=2^31-1)
    #[test]
    fn prop_invalid_initial_window_size_rejected(
        setting in invalid_initial_window_size(),
    ) {
        let mut settings = Settings::default();
        let result = settings.apply(setting);
        prop_assert!(
            result.is_err(),
            "Invalid INITIAL_WINDOW_SIZE {:?} should be rejected",
            setting
        );
    }

    /// 無効な MAX_FRAME_SIZE は拒否される
    ///
    /// 数学的意義: 範囲制約 (16384..=16777215)
    #[test]
    fn prop_invalid_max_frame_size_rejected(
        setting in invalid_max_frame_size(),
    ) {
        let mut settings = Settings::default();
        let result = settings.apply(setting);
        prop_assert!(
            result.is_err(),
            "Invalid MAX_FRAME_SIZE {:?} should be rejected",
            setting
        );
    }

    /// 無効な ENABLE_CONNECT_PROTOCOL は拒否される
    ///
    /// 数学的意義: 二値性の検証 (0 または 1 のみ)
    /// RFC 8441: ENABLE_CONNECT_PROTOCOL は 0 または 1 のみ有効
    #[test]
    fn prop_invalid_enable_connect_protocol_rejected(
        setting in invalid_enable_connect_protocol(),
    ) {
        let mut settings = Settings::default();
        let result = settings.apply(setting);
        prop_assert!(
            result.is_err(),
            "Invalid ENABLE_CONNECT_PROTOCOL {:?} should be rejected",
            setting
        );
    }

    /// 無効な NO_RFC7540_PRIORITIES は拒否される
    ///
    /// 数学的意義: 二値性の検証 (0 または 1 のみ)
    #[test]
    fn prop_invalid_no_rfc7540_priorities_rejected(
        setting in invalid_no_rfc7540_priorities(),
    ) {
        let mut settings = Settings::default();
        let result = settings.apply(setting);
        prop_assert!(
            result.is_err(),
            "Invalid NO_RFC7540_PRIORITIES {:?} should be rejected",
            setting
        );
    }

    /// 未知の SETTINGS は黙って無視される
    ///
    /// RFC 9113 Section 6.5.2: unknown settings MUST be ignored
    /// 数学的意義: 部分関数の安全な拡張
    #[test]
    fn prop_unknown_settings_ignored(
        unknown in unknown_setting(),
    ) {
        let mut settings = Settings::default();
        let original = settings.clone();

        let result = settings.apply(unknown);

        // 未知の SETTINGS は常に成功する
        prop_assert!(
            result.is_ok(),
            "Unknown setting {:?} should be silently ignored",
            unknown
        );

        // 既知のフィールドは変更されない
        prop_assert_eq!(settings.header_table_size, original.header_table_size);
        prop_assert_eq!(settings.enable_push, original.enable_push);
        prop_assert_eq!(settings.max_concurrent_streams, original.max_concurrent_streams);
        prop_assert_eq!(settings.initial_window_size, original.initial_window_size);
        prop_assert_eq!(settings.max_frame_size, original.max_frame_size);
        prop_assert_eq!(settings.max_header_list_size, original.max_header_list_size);
        prop_assert_eq!(settings.no_rfc7540_priorities, original.no_rfc7540_priorities);
    }

    /// 同じ SETTINGS を複数回適用しても結果は同じ
    ///
    /// 数学的意義: 冪等性
    #[test]
    fn prop_settings_idempotent(
        setting in valid_setting(),
        repeat_count in 2..10usize,
    ) {
        let mut settings = Settings::default();

        // 最初に適用
        settings.apply(setting).unwrap();
        let after_first = settings.clone();

        // 同じ設定を複数回適用
        for _ in 1..repeat_count {
            settings.apply(setting).unwrap();
        }

        // 最初の適用後と同じ状態
        prop_assert_eq!(settings, after_first);
    }

    /// ENABLE_PUSH は 0 または 1 のみ有効
    ///
    /// 数学的意義: 二値性
    #[test]
    fn prop_enable_push_binary(
        value in any::<u32>(),
    ) {
        let mut settings = Settings::default();
        let setting = Setting::from_setting_id(SettingId::EnablePush, value);
        let result = settings.apply(setting);

        if value <= 1 {
            prop_assert!(result.is_ok());
            prop_assert_eq!(settings.enable_push, value == 1);
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// MAX_FRAME_SIZE は 16384..=16777215 の範囲のみ有効
    ///
    /// 数学的意義: 範囲制約
    #[test]
    fn prop_max_frame_size_bounds(
        value in any::<u32>(),
    ) {
        let mut settings = Settings::default();
        let setting = Setting::from_setting_id(SettingId::MaxFrameSize, value);
        let result = settings.apply(setting);

        if (MIN_MAX_FRAME_SIZE..=MAX_MAX_FRAME_SIZE).contains(&value) {
            prop_assert!(result.is_ok());
            prop_assert_eq!(settings.max_frame_size, value);
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// INITIAL_WINDOW_SIZE は 0..=2147483647 の範囲のみ有効
    ///
    /// 数学的意義: 範囲制約
    #[test]
    fn prop_initial_window_size_bounds(
        value in any::<u32>(),
    ) {
        let mut settings = Settings::default();
        let setting = Setting::from_setting_id(SettingId::InitialWindowSize, value);
        let result = settings.apply(setting);

        if value <= MAX_INITIAL_WINDOW_SIZE {
            prop_assert!(result.is_ok());
            prop_assert_eq!(settings.initial_window_size, value);
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// デフォルト値の検証
    ///
    /// RFC 9113 Section 6.5.2 のデフォルト値
    /// RFC 9113 Section 8.4: サーバープッシュは主要ブラウザでサポートが削除されているため
    /// DEFAULT_ENABLE_PUSH は false
    #[test]
    fn prop_default_values(_dummy in Just(())) {
        let settings = Settings::default();

        prop_assert_eq!(settings.header_table_size, DEFAULT_HEADER_TABLE_SIZE);
        // RFC 9113 Section 8.4: サーバープッシュは非推奨のため、デフォルトは false
        prop_assert_eq!(settings.enable_push, DEFAULT_ENABLE_PUSH);
        prop_assert!(!settings.enable_push, "DEFAULT_ENABLE_PUSH should be false");
        prop_assert_eq!(settings.max_concurrent_streams, None);
        prop_assert_eq!(settings.initial_window_size, DEFAULT_INITIAL_WINDOW_SIZE);
        prop_assert_eq!(settings.max_frame_size, DEFAULT_MAX_FRAME_SIZE);
        prop_assert_eq!(settings.max_header_list_size, None);
        // RFC 8441: ENABLE_CONNECT_PROTOCOL のデフォルトは false
        prop_assert!(!settings.enable_connect_protocol);
        prop_assert!(!settings.no_rfc7540_priorities);
    }

    /// 複数の SETTINGS を順に適用した場合、最後の値が残る
    ///
    /// 数学的意義: 上書きセマンティクス
    #[test]
    fn prop_settings_last_wins(
        values in prop::collection::vec(0..=MAX_INITIAL_WINDOW_SIZE, 2..10),
    ) {
        let mut settings = Settings::default();

        for value in &values {
            let setting = Setting::from_setting_id(SettingId::InitialWindowSize, *value);
            settings.apply(setting).unwrap();
        }

        // 最後に適用した値が残る
        prop_assert_eq!(settings.initial_window_size, *values.last().unwrap());
    }

    /// to_settings_list と apply の整合性
    ///
    /// 数学的意義: シリアライズとデシリアライズの整合性
    #[test]
    fn prop_settings_roundtrip(
        header_table_size in any::<u32>(),
        enable_push in prop::bool::ANY,
        max_concurrent_streams in prop::option::of(any::<u32>()),
        initial_window_size in 0..=MAX_INITIAL_WINDOW_SIZE,
        max_frame_size in MIN_MAX_FRAME_SIZE..=MAX_MAX_FRAME_SIZE,
        max_header_list_size in prop::option::of(any::<u32>()),
    ) {
        // 元の設定を構築
        let mut original = Settings::default();
        original.apply(Setting::from_setting_id(SettingId::HeaderTableSize, header_table_size)).unwrap();
        original.apply(Setting::from_setting_id(SettingId::EnablePush, u32::from(enable_push))).unwrap();
        if let Some(v) = max_concurrent_streams {
            original.apply(Setting::from_setting_id(SettingId::MaxConcurrentStreams, v)).unwrap();
        }
        original.apply(Setting::from_setting_id(SettingId::InitialWindowSize, initial_window_size)).unwrap();
        original.apply(Setting::from_setting_id(SettingId::MaxFrameSize, max_frame_size)).unwrap();
        if let Some(v) = max_header_list_size {
            original.apply(Setting::from_setting_id(SettingId::MaxHeaderListSize, v)).unwrap();
        }

        // リストに変換して再適用
        let list = original.to_settings_list();
        let mut restored = Settings::default();
        for setting in list {
            restored.apply(setting).unwrap();
        }

        // 主要な値が一致することを確認
        prop_assert_eq!(restored.header_table_size, original.header_table_size);
        prop_assert_eq!(restored.enable_push, original.enable_push);
        prop_assert_eq!(restored.initial_window_size, original.initial_window_size);
        prop_assert_eq!(restored.max_frame_size, original.max_frame_size);
        // max_concurrent_streams は None から Some に変わる可能性があるため、
        // 明示的に設定した場合のみ比較
        if max_concurrent_streams.is_some() {
            prop_assert_eq!(restored.max_concurrent_streams, original.max_concurrent_streams);
        }
        if max_header_list_size.is_some() {
            prop_assert_eq!(restored.max_header_list_size, original.max_header_list_size);
        }
    }
}
