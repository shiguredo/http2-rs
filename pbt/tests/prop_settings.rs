//! SETTINGS パラメータの PBT (RFC 9113 Section 6.5.2)
//!
//! HTTP/2 SETTINGS パラメータの検証を行う。

use proptest::prelude::*;
use shiguredo_http2::settings::{
    DEFAULT_ENABLE_PUSH, DEFAULT_HEADER_TABLE_SIZE, DEFAULT_INITIAL_WINDOW_SIZE,
    DEFAULT_MAX_FRAME_SIZE, MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, MIN_MAX_FRAME_SIZE,
};
use shiguredo_http2::{MaxFrameSize, Setting, Settings, WindowSize};

/// 有効な SETTINGS を生成する
fn valid_setting() -> impl Strategy<Value = Setting> {
    prop_oneof![
        any::<u32>().prop_map(Setting::HeaderTableSize),
        prop::bool::ANY.prop_map(Setting::EnablePush),
        any::<u32>().prop_map(Setting::MaxConcurrentStreams),
        (0..=MAX_INITIAL_WINDOW_SIZE)
            .prop_map(|v| Setting::InitialWindowSize(WindowSize::new(v).unwrap())),
        (MIN_MAX_FRAME_SIZE..=MAX_MAX_FRAME_SIZE)
            .prop_map(|v| Setting::MaxFrameSize(MaxFrameSize::new(v).unwrap())),
        any::<u32>().prop_map(Setting::MaxHeaderListSize),
        prop::bool::ANY.prop_map(Setting::EnableConnectProtocol),
        prop::bool::ANY.prop_map(Setting::NoRfc7540Priorities),
        any::<u32>().prop_map(Setting::WtInitialMaxData),
        any::<u32>().prop_map(Setting::WtInitialMaxStreamDataUni),
        any::<u32>().prop_map(Setting::WtInitialMaxStreamDataBidiLocal),
        any::<u32>().prop_map(Setting::WtInitialMaxStreamsUni),
        any::<u32>().prop_map(Setting::WtInitialMaxStreamsBidi),
        any::<u32>().prop_map(Setting::WtInitialMaxStreamDataBidiRemote),
    ]
}

/// 無効な ENABLE_PUSH wire 値を生成 (2 以上)
fn invalid_enable_push_wire() -> impl Strategy<Value = (u16, u32)> {
    (2..=u32::MAX).prop_map(|v| (0x02, v))
}

/// 無効な INITIAL_WINDOW_SIZE wire 値を生成 (2^31 以上)
fn invalid_initial_window_size_wire() -> impl Strategy<Value = (u16, u32)> {
    ((MAX_INITIAL_WINDOW_SIZE + 1)..=u32::MAX).prop_map(|v| (0x04, v))
}

/// 無効な MAX_FRAME_SIZE wire 値を生成 (範囲外)
fn invalid_max_frame_size_wire() -> impl Strategy<Value = (u16, u32)> {
    prop_oneof![
        (0..MIN_MAX_FRAME_SIZE).prop_map(|v| (0x05u16, v)),
        ((MAX_MAX_FRAME_SIZE + 1)..=u32::MAX).prop_map(|v| (0x05u16, v)),
    ]
}

/// 無効な ENABLE_CONNECT_PROTOCOL wire 値を生成 (2 以上)
fn invalid_enable_connect_protocol_wire() -> impl Strategy<Value = (u16, u32)> {
    (2..=u32::MAX).prop_map(|v| (0x08, v))
}

/// 無効な NO_RFC7540_PRIORITIES wire 値を生成 (2 以上)
fn invalid_no_rfc7540_priorities_wire() -> impl Strategy<Value = (u16, u32)> {
    (2..=u32::MAX).prop_map(|v| (0x09, v))
}

/// 未知の SETTINGS ID を生成
fn unknown_setting_wire() -> impl Strategy<Value = (u16, u32)> {
    // 既知 ID (0x01-0x06, 0x08, 0x09, 0x2b61-0x2b66) を除外
    (0x0a..=0xffffu16, any::<u32>()).prop_filter("must not be a known ID", |(id, _)| {
        !matches!(*id, 0x2b61..=0x2b66)
    })
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
        settings.apply(setting);
        // apply は検証済み Setting を受け取るため、常に成功する
    }

    /// 無効な ENABLE_PUSH wire 値は from_wire で拒否される
    ///
    /// 数学的意義: 二値性の検証 (0 または 1 のみ)
    #[test]
    fn prop_invalid_enable_push_rejected(
        (id, value) in invalid_enable_push_wire(),
    ) {
        let result = Setting::from_wire(id, value);
        prop_assert!(
            result.is_err(),
            "Invalid ENABLE_PUSH wire ({id}, {value}) should be rejected",
        );
    }

    /// 無効な INITIAL_WINDOW_SIZE wire 値は from_wire で拒否される
    ///
    /// 数学的意義: 範囲制約 (0..=2^31-1)
    #[test]
    fn prop_invalid_initial_window_size_rejected(
        (id, value) in invalid_initial_window_size_wire(),
    ) {
        let result = Setting::from_wire(id, value);
        prop_assert!(
            result.is_err(),
            "Invalid INITIAL_WINDOW_SIZE wire ({id}, {value}) should be rejected",
        );
    }

    /// 無効な MAX_FRAME_SIZE wire 値は from_wire で拒否される
    ///
    /// 数学的意義: 範囲制約 (16384..=16777215)
    #[test]
    fn prop_invalid_max_frame_size_rejected(
        (id, value) in invalid_max_frame_size_wire(),
    ) {
        let result = Setting::from_wire(id, value);
        prop_assert!(
            result.is_err(),
            "Invalid MAX_FRAME_SIZE wire ({id}, {value}) should be rejected",
        );
    }

    /// 無効な ENABLE_CONNECT_PROTOCOL wire 値は from_wire で拒否される
    ///
    /// 数学的意義: 二値性の検証 (0 または 1 のみ)
    /// RFC 8441: ENABLE_CONNECT_PROTOCOL は 0 または 1 のみ有効
    #[test]
    fn prop_invalid_enable_connect_protocol_rejected(
        (id, value) in invalid_enable_connect_protocol_wire(),
    ) {
        let result = Setting::from_wire(id, value);
        prop_assert!(
            result.is_err(),
            "Invalid ENABLE_CONNECT_PROTOCOL wire ({id}, {value}) should be rejected",
        );
    }

    /// 無効な NO_RFC7540_PRIORITIES wire 値は from_wire で拒否される
    ///
    /// 数学的意義: 二値性の検証 (0 または 1 のみ)
    #[test]
    fn prop_invalid_no_rfc7540_priorities_rejected(
        (id, value) in invalid_no_rfc7540_priorities_wire(),
    ) {
        let result = Setting::from_wire(id, value);
        prop_assert!(
            result.is_err(),
            "Invalid NO_RFC7540_PRIORITIES wire ({id}, {value}) should be rejected",
        );
    }

    /// 未知の SETTINGS wire 値は from_wire で Unknown として受理される
    ///
    /// RFC 9113 Section 6.5.2: unknown settings MUST be ignored
    /// 数学的意義: 部分関数の安全な拡張
    #[test]
    fn prop_unknown_settings_ignored(
        (id, value) in unknown_setting_wire(),
    ) {
        let result = Setting::from_wire(id, value);
        prop_assert!(
            result.is_ok(),
            "Unknown setting wire ({id}, {value}) should produce Ok(Unknown)",
        );
        let setting = result.unwrap();
        prop_assert!(
            matches!(setting, Setting::Unknown { .. }),
            "Unknown ID should produce Setting::Unknown, got {:?}",
            setting,
        );

        // apply しても既知フィールドは変更されない
        let mut settings = Settings::default();
        let original = settings.clone();
        settings.apply(setting);

        prop_assert_eq!(settings.header_table_size(), original.header_table_size());
        prop_assert_eq!(settings.enable_push(), original.enable_push());
        prop_assert_eq!(settings.max_concurrent_streams(), original.max_concurrent_streams());
        prop_assert_eq!(settings.initial_window_size(), original.initial_window_size());
        prop_assert_eq!(settings.max_frame_size(), original.max_frame_size());
        prop_assert_eq!(settings.max_header_list_size(), original.max_header_list_size());
        prop_assert_eq!(settings.enable_connect_protocol(), original.enable_connect_protocol());
        prop_assert_eq!(settings.no_rfc7540_priorities(), original.no_rfc7540_priorities());
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

        settings.apply(setting);
        let after_first = settings.clone();

        for _ in 1..repeat_count {
            settings.apply(setting);
        }

        prop_assert_eq!(settings, after_first);
    }

    /// ENABLE_PUSH wire 値は 0 または 1 のみ有効
    ///
    /// 数学的意義: 二値性
    #[test]
    fn prop_enable_push_binary(
        value in any::<u32>(),
    ) {
        let result = Setting::from_wire(0x02, value);

        if value <= 1 {
            prop_assert!(result.is_ok());
            let mut settings = Settings::default();
            settings.apply(result.unwrap());
            prop_assert_eq!(settings.enable_push(), value == 1);
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// MAX_FRAME_SIZE wire 値は 16384..=16777215 の範囲のみ有効
    ///
    /// 数学的意義: 範囲制約
    #[test]
    fn prop_max_frame_size_bounds(
        value in any::<u32>(),
    ) {
        let result = Setting::from_wire(0x05, value);

        if (MIN_MAX_FRAME_SIZE..=MAX_MAX_FRAME_SIZE).contains(&value) {
            prop_assert!(result.is_ok());
            let mut settings = Settings::default();
            settings.apply(result.unwrap());
            prop_assert_eq!(settings.max_frame_size().get(), value);
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// INITIAL_WINDOW_SIZE wire 値は 0..=2147483647 の範囲のみ有効
    ///
    /// 数学的意義: 範囲制約
    #[test]
    fn prop_initial_window_size_bounds(
        value in any::<u32>(),
    ) {
        let result = Setting::from_wire(0x04, value);

        if value <= MAX_INITIAL_WINDOW_SIZE {
            prop_assert!(result.is_ok());
            let mut settings = Settings::default();
            settings.apply(result.unwrap());
            prop_assert_eq!(settings.initial_window_size().get(), value);
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// デフォルト値の検証
    ///
    /// RFC 9113 Section 6.5.2 のデフォルト値
    #[test]
    fn prop_default_values(_dummy in Just(())) {
        let settings = Settings::default();

        prop_assert_eq!(settings.header_table_size(), DEFAULT_HEADER_TABLE_SIZE);
        prop_assert_eq!(settings.enable_push(), DEFAULT_ENABLE_PUSH);
        prop_assert!(!settings.enable_push(), "DEFAULT_ENABLE_PUSH should be false");
        prop_assert_eq!(settings.max_concurrent_streams(), None);
        prop_assert_eq!(settings.initial_window_size().get(), DEFAULT_INITIAL_WINDOW_SIZE);
        prop_assert_eq!(settings.max_frame_size().get(), DEFAULT_MAX_FRAME_SIZE);
        prop_assert_eq!(settings.max_header_list_size(), None);
        prop_assert!(!settings.enable_connect_protocol());
        prop_assert!(!settings.no_rfc7540_priorities());
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
            let setting = Setting::from_wire(0x04, *value).unwrap();
            settings.apply(setting);
        }

        prop_assert_eq!(settings.initial_window_size().get(), *values.last().unwrap());
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
        enable_connect_protocol in prop::bool::ANY,
        no_rfc7540_priorities in prop::bool::ANY,
        wt_max_data in prop::option::of(any::<u32>()),
    ) {
        let mut original = Settings::default();
        original.apply(Setting::HeaderTableSize(header_table_size));
        original.apply(Setting::EnablePush(enable_push));
        if let Some(v) = max_concurrent_streams {
            original.apply(Setting::MaxConcurrentStreams(v));
        }
        original.apply(Setting::InitialWindowSize(WindowSize::new(initial_window_size).unwrap()));
        original.apply(Setting::MaxFrameSize(MaxFrameSize::new(max_frame_size).unwrap()));
        if let Some(v) = max_header_list_size {
            original.apply(Setting::MaxHeaderListSize(v));
        }
        if enable_connect_protocol {
            original.apply(Setting::EnableConnectProtocol(true));
        }
        if no_rfc7540_priorities {
            original.apply(Setting::NoRfc7540Priorities(true));
        }
        if let Some(v) = wt_max_data {
            original.apply(Setting::WtInitialMaxData(v));
        }

        let list = original.to_settings_list();
        let mut restored = Settings::default();
        for setting in list {
            restored.apply(setting);
        }

        prop_assert_eq!(restored.header_table_size(), original.header_table_size());
        prop_assert_eq!(restored.enable_push(), original.enable_push());
        prop_assert_eq!(restored.initial_window_size(), original.initial_window_size());
        prop_assert_eq!(restored.max_frame_size(), original.max_frame_size());
        prop_assert_eq!(restored.enable_connect_protocol(), original.enable_connect_protocol());
        prop_assert_eq!(restored.no_rfc7540_priorities(), original.no_rfc7540_priorities());
        if max_concurrent_streams.is_some() {
            prop_assert_eq!(restored.max_concurrent_streams(), original.max_concurrent_streams());
        }
        if max_header_list_size.is_some() {
            prop_assert_eq!(restored.max_header_list_size(), original.max_header_list_size());
        }
        if wt_max_data.is_some() {
            prop_assert_eq!(restored.wt_initial_max_data(), original.wt_initial_max_data());
        }
    }

    /// Setting::from_wire と Setting::as_wire の往復
    ///
    /// 数学的意義: wire 変換のラウンドトリップ
    #[test]
    fn prop_setting_wire_roundtrip(
        setting in valid_setting(),
    ) {
        let (id, value) = setting.as_wire();
        let restored = Setting::from_wire(id, value).unwrap();
        prop_assert_eq!(setting, restored);
    }

    /// WindowSize::from_static と WindowSize::new の一貫性
    ///
    /// from_static が成功するリテラルは new でも同じ結果を返す
    #[test]
    fn prop_window_size_static_matches_new(
        size in 0u32..=MAX_INITIAL_WINDOW_SIZE,
    ) {
        let via_new = WindowSize::new(size).unwrap();
        let via_static = WindowSize::from_static(size);
        prop_assert_eq!(via_new, via_static);
    }

    /// MaxFrameSize::from_static と MaxFrameSize::new の一貫性
    #[test]
    fn prop_max_frame_size_static_matches_new(
        size in MIN_MAX_FRAME_SIZE..=MAX_MAX_FRAME_SIZE,
    ) {
        let via_new = MaxFrameSize::new(size).unwrap();
        let via_static = MaxFrameSize::from_static(size);
        prop_assert_eq!(via_new, via_static);
    }
}
