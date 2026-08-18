//! SETTINGS パラメータの PBT (RFC 9113 Section 6.5.2)
//!
//! HTTP/2 SETTINGS パラメータの検証を行う。

use shiguredo_http2::settings::{MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, MIN_MAX_FRAME_SIZE};
use shiguredo_http2::{MaxFrameSize, Setting, Settings, WindowSize};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// 有効な INITIAL_WINDOW_SIZE 値を、0 と上限に 1/5 の確率を付けて引く
fn sample_window_size_value(ctx: &mut noprop::TestCaseContext) -> u32 {
    noprop::sample_with_boundaries(
        ctx,
        &[0u32, MAX_INITIAL_WINDOW_SIZE],
        noprop::Ratio::one_nth(5),
        |ctx| noprop::sample_u64_in(ctx, 0..=MAX_INITIAL_WINDOW_SIZE as u64) as u32,
    )
}

/// 有効な MAX_FRAME_SIZE 値を、下限と上限に 1/5 の確率を付けて引く
fn sample_max_frame_size_value(ctx: &mut noprop::TestCaseContext) -> u32 {
    noprop::sample_with_boundaries(
        ctx,
        &[MIN_MAX_FRAME_SIZE, MAX_MAX_FRAME_SIZE],
        noprop::Ratio::one_nth(5),
        |ctx| {
            noprop::sample_u64_in(ctx, MIN_MAX_FRAME_SIZE as u64..=MAX_MAX_FRAME_SIZE as u64) as u32
        },
    )
}

/// 有効な SETTINGS を生成する
fn sample_valid_setting(ctx: &mut noprop::TestCaseContext) -> Setting {
    match noprop::sample_weighted_index(ctx, &[1; 15]) {
        0 => Setting::HeaderTableSize(noprop::sample_u32(ctx)),
        1 => Setting::EnablePush(noprop::sample_bool(ctx)),
        2 => Setting::MaxConcurrentStreams(noprop::sample_u32(ctx)),
        3 => Setting::InitialWindowSize(
            WindowSize::new(sample_window_size_value(ctx)).expect("valid SETTINGS value"),
        ),
        4 => Setting::MaxFrameSize(
            MaxFrameSize::new(sample_max_frame_size_value(ctx)).expect("valid SETTINGS value"),
        ),
        5 => Setting::MaxHeaderListSize(noprop::sample_u32(ctx)),
        6 => Setting::EnableConnectProtocol(noprop::sample_bool(ctx)),
        7 => Setting::NoRfc7540Priorities(noprop::sample_bool(ctx)),
        8 => Setting::WtEnabled(noprop::sample_bool(ctx)),
        9 => Setting::WtInitialMaxData(noprop::sample_u32(ctx)),
        10 => Setting::WtInitialMaxStreamDataUni(noprop::sample_u32(ctx)),
        11 => Setting::WtInitialMaxStreamDataBidiLocal(noprop::sample_u32(ctx)),
        12 => Setting::WtInitialMaxStreamsUni(noprop::sample_u32(ctx)),
        13 => Setting::WtInitialMaxStreamsBidi(noprop::sample_u32(ctx)),
        _ => Setting::WtInitialMaxStreamDataBidiRemote(noprop::sample_u32(ctx)),
    }
}

/// 2..=u32::MAX の値を生成する (二値フラグの無効な wire 値)
fn sample_invalid_bool_wire(ctx: &mut noprop::TestCaseContext) -> u32 {
    2 + noprop::sample_u64_in(ctx, 0..(u32::MAX as u64 - 1)) as u32
}

/// 無効な ENABLE_PUSH wire 値を生成 (2 以上)
fn sample_invalid_enable_push_wire(ctx: &mut noprop::TestCaseContext) -> (u16, u32) {
    (0x02, sample_invalid_bool_wire(ctx))
}

/// 無効な INITIAL_WINDOW_SIZE wire 値を生成 (2^31 以上)
fn sample_invalid_initial_window_size_wire(ctx: &mut noprop::TestCaseContext) -> (u16, u32) {
    (
        0x04,
        (MAX_INITIAL_WINDOW_SIZE + 1)
            + noprop::sample_u64_in(ctx, 0..(u32::MAX as u64 - MAX_INITIAL_WINDOW_SIZE as u64))
                as u32,
    )
}

/// 無効な MAX_FRAME_SIZE wire 値を生成 (範囲外)
fn sample_invalid_max_frame_size_wire(ctx: &mut noprop::TestCaseContext) -> (u16, u32) {
    match noprop::sample_weighted_index(ctx, &[1, 1]) {
        0 => (
            0x05,
            noprop::sample_with_boundaries(
                ctx,
                &[0u32, MIN_MAX_FRAME_SIZE - 1],
                noprop::Ratio::one_nth(5),
                |ctx| noprop::sample_u64_in(ctx, 0..MIN_MAX_FRAME_SIZE as u64) as u32,
            ),
        ),
        _ => (
            0x05,
            noprop::sample_with_boundaries(
                ctx,
                &[MAX_MAX_FRAME_SIZE + 1, u32::MAX],
                noprop::Ratio::one_nth(5),
                |ctx| {
                    (MAX_MAX_FRAME_SIZE + 1)
                        + noprop::sample_u64_in(
                            ctx,
                            0..(u32::MAX as u64 - MAX_MAX_FRAME_SIZE as u64),
                        ) as u32
                },
            ),
        ),
    }
}

/// 無効な ENABLE_CONNECT_PROTOCOL wire 値を生成 (2 以上)
fn sample_invalid_enable_connect_protocol_wire(ctx: &mut noprop::TestCaseContext) -> (u16, u32) {
    (0x08, sample_invalid_bool_wire(ctx))
}

/// 無効な NO_RFC7540_PRIORITIES wire 値を生成 (2 以上)
fn sample_invalid_no_rfc7540_priorities_wire(ctx: &mut noprop::TestCaseContext) -> (u16, u32) {
    (0x09, sample_invalid_bool_wire(ctx))
}

/// 無効な WT_ENABLED wire 値を生成 (2 以上)
fn sample_invalid_wt_enabled_wire(ctx: &mut noprop::TestCaseContext) -> (u16, u32) {
    (0x2b60, sample_invalid_bool_wire(ctx))
}

/// 未知の SETTINGS ID と任意の wire 値を生成する
///
/// 既知 ID (0x01-0x06, 0x08, 0x09, 0x2b60-0x2b66) を除外し、谷間のどこを
/// 引くかを一様なインデックスで valid-by-construction に決定する。
fn sample_unknown_setting_wire(ctx: &mut noprop::TestCaseContext) -> (u16, u32) {
    // 0x0a..=0x2b5f の個数 (0x2b60 未満の未知 ID 領域)
    const HEAD: usize = 0x2b60 - 0x0a;
    // 0x2b67..=0xffff の個数 (0x2b66 超の未知 ID 領域)
    const TAIL: usize = 0xffff - 0x2b66;
    let pick = noprop::sample_usize_in(ctx, 0..HEAD + TAIL);
    let id = if pick < HEAD {
        0x0a + pick
    } else {
        0x2b67 + (pick - HEAD)
    };
    (id as u16, noprop::sample_u32(ctx))
}

/// 無効な ENABLE_PUSH wire 値は from_wire で拒否される (RFC 9113 Section 6.5.2: 0/1 以外は PROTOCOL_ERROR)
///
/// 数学的意義: 二値性の検証 (0 または 1 のみ)
#[test]
fn prop_invalid_enable_push_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let (id, value) = sample_invalid_enable_push_wire(ctx);
        let result = Setting::from_wire(id, value);
        assert!(
            result.is_err(),
            "Invalid ENABLE_PUSH wire ({id}, {value}) should be rejected",
        );
        Ok(())
    })?;
    Ok(())
}

/// 無効な INITIAL_WINDOW_SIZE wire 値は from_wire で拒否される
///
/// 数学的意義: 範囲制約 (0..=2^31-1)
#[test]
fn prop_invalid_initial_window_size_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let (id, value) = sample_invalid_initial_window_size_wire(ctx);
        let result = Setting::from_wire(id, value);
        assert!(
            result.is_err(),
            "Invalid INITIAL_WINDOW_SIZE wire ({id}, {value}) should be rejected",
        );
        Ok(())
    })?;
    Ok(())
}

/// 無効な MAX_FRAME_SIZE wire 値は from_wire で拒否される
///
/// 数学的意義: 範囲制約 (16384..=16777215)
#[test]
fn prop_invalid_max_frame_size_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let (id, value) = sample_invalid_max_frame_size_wire(ctx);
        let result = Setting::from_wire(id, value);
        assert!(
            result.is_err(),
            "Invalid MAX_FRAME_SIZE wire ({id}, {value}) should be rejected",
        );
        Ok(())
    })?;
    Ok(())
}

/// 無効な ENABLE_CONNECT_PROTOCOL wire 値は from_wire で拒否される
///
/// 数学的意義: 二値性の検証 (0 または 1 のみ)
/// RFC 8441: ENABLE_CONNECT_PROTOCOL は 0 または 1 のみ有効
#[test]
fn prop_invalid_enable_connect_protocol_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let (id, value) = sample_invalid_enable_connect_protocol_wire(ctx);
        let result = Setting::from_wire(id, value);
        assert!(
            result.is_err(),
            "Invalid ENABLE_CONNECT_PROTOCOL wire ({id}, {value}) should be rejected",
        );
        Ok(())
    })?;
    Ok(())
}

/// 無効な NO_RFC7540_PRIORITIES wire 値は from_wire で拒否される
///
/// 数学的意義: 二値性の検証 (0 または 1 のみ)
/// RFC 9218 Section 2.1: NO_RFC7540_PRIORITIES の値は 0 または 1 でなければならない (MUST)。
#[test]
fn prop_invalid_no_rfc7540_priorities_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let (id, value) = sample_invalid_no_rfc7540_priorities_wire(ctx);
        let result = Setting::from_wire(id, value);
        assert!(
            result.is_err(),
            "Invalid NO_RFC7540_PRIORITIES wire ({id}, {value}) should be rejected",
        );
        Ok(())
    })?;
    Ok(())
}

/// 無効な WT_ENABLED wire 値は from_wire で拒否される
///
/// 数学的意義: 二値性の検証 (0 または 1 のみ)
/// draft-ietf-webtrans-http2-15 Section 3.1: クライアントは 1 より大きい値を
/// 接続エラー PROTOCOL_ERROR として扱わなければならない (MUST)。
#[test]
fn prop_invalid_wt_enabled_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let (id, value) = sample_invalid_wt_enabled_wire(ctx);
        let result = Setting::from_wire(id, value);
        assert!(
            result.is_err(),
            "Invalid WT_ENABLED wire ({id}, {value}) should be rejected",
        );
        Ok(())
    })?;
    Ok(())
}

/// 未知の SETTINGS wire 値は from_wire で Unknown として受理される
///
/// RFC 9113 Section 6.5.2: unknown settings MUST be ignored
/// 数学的意義: 部分関数の安全な拡張
#[test]
fn prop_unknown_settings_ignored() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let (id, value) = sample_unknown_setting_wire(ctx);
        let result = Setting::from_wire(id, value);
        assert!(
            result.is_ok(),
            "Unknown setting wire ({id}, {value}) should produce Ok(Unknown)",
        );
        let setting = result.expect("should succeed");
        assert!(
            matches!(setting, Setting::Unknown { .. }),
            "Unknown ID should produce Setting::Unknown, got {setting:?}",
        );

        // apply しても既知フィールドは変更されない
        let mut settings = Settings::default();
        let original = settings.clone();
        settings.apply(setting);

        assert_eq!(settings.header_table_size(), original.header_table_size());
        assert_eq!(settings.enable_push(), original.enable_push());
        assert_eq!(
            settings.max_concurrent_streams(),
            original.max_concurrent_streams()
        );
        assert_eq!(
            settings.initial_window_size(),
            original.initial_window_size()
        );
        assert_eq!(settings.max_frame_size(), original.max_frame_size());
        assert_eq!(
            settings.max_header_list_size(),
            original.max_header_list_size()
        );
        assert_eq!(
            settings.enable_connect_protocol(),
            original.enable_connect_protocol()
        );
        assert_eq!(
            settings.no_rfc7540_priorities(),
            original.no_rfc7540_priorities()
        );
        assert_eq!(settings.wt_enabled(), original.wt_enabled());
        Ok(())
    })?;
    Ok(())
}

/// 同じ SETTINGS を複数回適用しても結果は同じ
///
/// 数学的意義: 冪等性
#[test]
fn prop_settings_idempotent() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let setting = sample_valid_setting(ctx);
        let repeat_count = noprop::sample_usize_in(ctx, 2..=9);
        let mut settings = Settings::default();

        settings.apply(setting);
        let after_first = settings.clone();

        for _ in 1..repeat_count {
            settings.apply(setting);
        }

        assert_eq!(settings, after_first);
        Ok(())
    })?;
    Ok(())
}

/// ENABLE_PUSH wire 値は 0 または 1 のみ有効 (RFC 9113 Section 6.5.2: 0/1 以外は PROTOCOL_ERROR)
///
/// 数学的意義: 二値性
///
/// `any::<u32>()` 相当の一様では value<=1 の確率は 2/2^32 で成功パスに到達しない。
/// 有効 (0/1) と無効 (2..=MAX) を等確率の first-class 分岐にする。
/// N=256 で各分岐未到達は (1/2)^256。
#[test]
fn prop_enable_push_binary() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let ok_gate = std::cell::Cell::new(0usize);
    let err_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        match noprop::sample_weighted_index(ctx, &[1, 1]) {
            0 => {
                let value = noprop::sample_u64_in(ctx, 0..=1) as u32;
                let setting = Setting::from_wire(0x02, value).expect("0/1 is valid ENABLE_PUSH");
                let mut settings = Settings::default();
                settings.apply(setting);
                assert_eq!(settings.enable_push(), value == 1);
                ok_gate.set(ok_gate.get() + 1);
            }
            _ => {
                let value = sample_invalid_bool_wire(ctx);
                assert!(Setting::from_wire(0x02, value).is_err());
                err_gate.set(err_gate.get() + 1);
            }
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "ENABLE_PUSH の受理パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        err_gate.get() > 0,
        "ENABLE_PUSH の拒否パスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// MAX_FRAME_SIZE wire 値は 16384..=16777215 の範囲のみ有効
///
/// 数学的意義: 範囲制約
///
/// 有効範囲は u32 全体の約 0.4% なので一様 u32 では N=256 で受理パスを
/// 約 36% の確率で外す。過小 / 有効 / 過大を等確率の first-class 分岐にする。
/// 各分岐未到達は (2/3)^256 ≈ 1.4e-47。
#[test]
fn prop_max_frame_size_bounds() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let ok_gate = std::cell::Cell::new(0usize);
    let too_small_gate = std::cell::Cell::new(0usize);
    let too_large_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        match noprop::sample_weighted_index(ctx, &[1, 1, 1]) {
            0 => {
                let value = sample_max_frame_size_value(ctx);
                let setting = Setting::from_wire(0x05, value).expect("in-range MAX_FRAME_SIZE");
                let mut settings = Settings::default();
                settings.apply(setting);
                assert_eq!(settings.max_frame_size().get(), value);
                ok_gate.set(ok_gate.get() + 1);
            }
            1 => {
                let value = noprop::sample_with_boundaries(
                    ctx,
                    &[0u32, MIN_MAX_FRAME_SIZE - 1],
                    noprop::Ratio::one_nth(5),
                    |ctx| noprop::sample_u64_in(ctx, 0..MIN_MAX_FRAME_SIZE as u64) as u32,
                );
                assert!(Setting::from_wire(0x05, value).is_err());
                too_small_gate.set(too_small_gate.get() + 1);
            }
            _ => {
                let value = noprop::sample_with_boundaries(
                    ctx,
                    &[MAX_MAX_FRAME_SIZE + 1, u32::MAX],
                    noprop::Ratio::one_nth(5),
                    |ctx| {
                        (MAX_MAX_FRAME_SIZE + 1)
                            + noprop::sample_u64_in(
                                ctx,
                                0..(u32::MAX as u64 - MAX_MAX_FRAME_SIZE as u64),
                            ) as u32
                    },
                );
                assert!(Setting::from_wire(0x05, value).is_err());
                too_large_gate.set(too_large_gate.get() + 1);
            }
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "MAX_FRAME_SIZE の受理パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        too_small_gate.get() > 0,
        "MAX_FRAME_SIZE 過小の拒否パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        too_large_gate.get() > 0,
        "MAX_FRAME_SIZE 過大の拒否パスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// INITIAL_WINDOW_SIZE wire 値は 0..=2147483647 の範囲のみ有効
///
/// 数学的意義: 範囲制約
///
/// 有効/無効を等確率の first-class 分岐にする (各 1/2、N=256 で未到達は (1/2)^256)。
#[test]
fn prop_initial_window_size_bounds() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let ok_gate = std::cell::Cell::new(0usize);
    let err_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        match noprop::sample_weighted_index(ctx, &[1, 1]) {
            0 => {
                let value = sample_window_size_value(ctx);
                let setting =
                    Setting::from_wire(0x04, value).expect("in-range INITIAL_WINDOW_SIZE");
                let mut settings = Settings::default();
                settings.apply(setting);
                assert_eq!(settings.initial_window_size().get(), value);
                ok_gate.set(ok_gate.get() + 1);
            }
            _ => {
                let value = (MAX_INITIAL_WINDOW_SIZE + 1)
                    + noprop::sample_u64_in(
                        ctx,
                        0..(u32::MAX as u64 - MAX_INITIAL_WINDOW_SIZE as u64),
                    ) as u32;
                assert!(Setting::from_wire(0x04, value).is_err());
                err_gate.set(err_gate.get() + 1);
            }
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "INITIAL_WINDOW_SIZE の受理パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        err_gate.get() > 0,
        "INITIAL_WINDOW_SIZE の拒否パスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// 複数の SETTINGS を順に適用した場合、最後の値が残る
///
/// 数学的意義: 上書きセマンティクス
#[test]
fn prop_settings_last_wins() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let count =
            noprop::sample_with_boundaries(ctx, &[2usize, 9], noprop::Ratio::one_nth(5), |ctx| {
                noprop::sample_usize_in(ctx, 2..=9)
            });
        let values: Vec<u32> = (0..count).map(|_| sample_window_size_value(ctx)).collect();
        let mut settings = Settings::default();

        for value in &values {
            let setting = Setting::from_wire(0x04, *value).expect("construction should succeed");
            settings.apply(setting);
        }

        assert_eq!(
            settings.initial_window_size().get(),
            *values.last().expect("collection should be non-empty")
        );
        Ok(())
    })?;
    Ok(())
}

/// to_settings_list と apply の整合性
///
/// 数学的意義: シリアライズとデシリアライズの整合性
#[test]
fn prop_settings_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let header_table_size = noprop::sample_u32(ctx);
        let enable_push = noprop::sample_bool(ctx);
        let max_concurrent_streams = if noprop::sample_bool(ctx) {
            Some(noprop::sample_u32(ctx))
        } else {
            None
        };
        let initial_window_size = sample_window_size_value(ctx);
        let max_frame_size = sample_max_frame_size_value(ctx);
        let max_header_list_size = if noprop::sample_bool(ctx) {
            Some(noprop::sample_u32(ctx))
        } else {
            None
        };
        let enable_connect_protocol = noprop::sample_bool(ctx);
        let no_rfc7540_priorities = noprop::sample_bool(ctx);
        let wt_enabled = noprop::sample_bool(ctx);
        let wt_max_data = if noprop::sample_bool(ctx) {
            Some(noprop::sample_u32(ctx))
        } else {
            None
        };

        let mut original = Settings::default();
        original.apply(Setting::HeaderTableSize(header_table_size));
        original.apply(Setting::EnablePush(enable_push));
        if let Some(v) = max_concurrent_streams {
            original.apply(Setting::MaxConcurrentStreams(v));
        }
        original.apply(Setting::InitialWindowSize(
            WindowSize::new(initial_window_size).expect("valid SETTINGS value"),
        ));
        original.apply(Setting::MaxFrameSize(
            MaxFrameSize::new(max_frame_size).expect("valid SETTINGS value"),
        ));
        if let Some(v) = max_header_list_size {
            original.apply(Setting::MaxHeaderListSize(v));
        }
        if enable_connect_protocol {
            original.apply(Setting::EnableConnectProtocol(true));
        }
        if no_rfc7540_priorities {
            original.apply(Setting::NoRfc7540Priorities(true));
        }
        if wt_enabled {
            original.apply(Setting::WtEnabled(true));
        }
        if let Some(v) = wt_max_data {
            original.apply(Setting::WtInitialMaxData(v));
        }

        let list = original.to_settings_list();
        let mut restored = Settings::default();
        for setting in list {
            restored.apply(setting);
        }

        assert_eq!(restored.header_table_size(), original.header_table_size());
        assert_eq!(restored.enable_push(), original.enable_push());
        assert_eq!(
            restored.initial_window_size(),
            original.initial_window_size()
        );
        assert_eq!(restored.max_frame_size(), original.max_frame_size());
        assert_eq!(
            restored.enable_connect_protocol(),
            original.enable_connect_protocol()
        );
        assert_eq!(
            restored.no_rfc7540_priorities(),
            original.no_rfc7540_priorities()
        );
        assert_eq!(restored.wt_enabled(), original.wt_enabled());
        if max_concurrent_streams.is_some() {
            assert_eq!(
                restored.max_concurrent_streams(),
                original.max_concurrent_streams()
            );
        }
        if max_header_list_size.is_some() {
            assert_eq!(
                restored.max_header_list_size(),
                original.max_header_list_size()
            );
        }
        if wt_max_data.is_some() {
            assert_eq!(
                restored.wt_initial_max_data(),
                original.wt_initial_max_data()
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// Setting::from_wire と Setting::as_wire の往復
///
/// 数学的意義: wire 変換のラウンドトリップ
#[test]
fn prop_setting_wire_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let setting = sample_valid_setting(ctx);
        let (id, value) = setting.as_wire();
        let restored = Setting::from_wire(id, value).expect("construction should succeed");
        assert_eq!(setting, restored);
        Ok(())
    })?;
    Ok(())
}

/// WindowSize::from_static と WindowSize::new の一貫性
///
/// from_static が成功するリテラルは new でも同じ結果を返す
#[test]
fn prop_window_size_static_matches_new() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let size = sample_window_size_value(ctx);
        let via_new = WindowSize::new(size).expect("valid SETTINGS value");
        let via_static = WindowSize::from_static(size);
        assert_eq!(via_new, via_static);
        Ok(())
    })?;
    Ok(())
}

/// MaxFrameSize::from_static と MaxFrameSize::new の一貫性
#[test]
fn prop_max_frame_size_static_matches_new() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let size = sample_max_frame_size_value(ctx);
        let via_new = MaxFrameSize::new(size).expect("valid SETTINGS value");
        let via_static = MaxFrameSize::from_static(size);
        assert_eq!(via_new, via_static);
        Ok(())
    })?;
    Ok(())
}
