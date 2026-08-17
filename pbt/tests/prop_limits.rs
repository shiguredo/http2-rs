//! Limits ビルダーの PBT
//!
//! LimitsBuilder::build() の複合制約検査を検証する。

use shiguredo_http2::settings::{
    DEFAULT_INITIAL_WINDOW_SIZE, MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, MIN_MAX_FRAME_SIZE,
};
use shiguredo_http2::{Limits, LimitsError, MaxFrameSize, WindowSize};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// Option<u32> を生成する
fn sample_option_u32(ctx: &mut noprop::TestCaseContext) -> Option<u32> {
    if noprop::sample_bool(ctx) {
        Some(noprop::sample_u32(ctx))
    } else {
        None
    }
}

/// 接続レベルの有効なウィンドウサイズを生成する
///
/// RFC 9113 Section 6.9.2: 接続レベルのウィンドウは SETTINGS では縮小できないため、
/// `LimitsBuilder::connection_window_size` は `DEFAULT_INITIAL_WINDOW_SIZE` 未満を拒否する。
fn sample_valid_connection_window_size(ctx: &mut noprop::TestCaseContext) -> WindowSize {
    WindowSize::from_static(
        DEFAULT_INITIAL_WINDOW_SIZE
            + noprop::sample_u64_in(
                ctx,
                0..=(MAX_INITIAL_WINDOW_SIZE - DEFAULT_INITIAL_WINDOW_SIZE) as u64,
            ) as u32,
    )
}

/// 有効な LimitsBuilder 設定で build は常に成功する (WT なし)
#[test]
fn prop_valid_limits_build_succeeds() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_concurrent = sample_option_u32(ctx);
        let initial_window = WindowSize::from_static(noprop::sample_u64_in(
            ctx,
            0..=MAX_INITIAL_WINDOW_SIZE as u64,
        ) as u32);
        let max_frame = MaxFrameSize::from_static(noprop::sample_u64_in(
            ctx,
            MIN_MAX_FRAME_SIZE as u64..=MAX_MAX_FRAME_SIZE as u64,
        ) as u32);
        let max_header_list = sample_option_u32(ctx);
        let header_table = noprop::sample_u32(ctx);
        let connection_window = sample_valid_connection_window_size(ctx);
        let enable_connect = noprop::sample_bool(ctx);
        let no_rfc7540 = noprop::sample_bool(ctx);

        let result = Limits::builder()
            .max_concurrent_streams(max_concurrent)
            .initial_window_size(initial_window)
            .max_frame_size(max_frame)
            .max_header_list_size(max_header_list)
            .header_table_size(header_table)
            .connection_window_size(connection_window)
            .enable_connect_protocol(enable_connect)
            .no_rfc7540_priorities(no_rfc7540)
            .build();
        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

/// WebTransport 設定 + enable_connect_protocol=true + wt_enabled=true で build は成功する
#[test]
fn prop_wt_with_connect_protocol_succeeds() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_data = sample_option_u32(ctx);
        let max_stream_uni = sample_option_u32(ctx);
        let max_stream_bidi_local = sample_option_u32(ctx);
        let max_streams_uni = sample_option_u32(ctx);
        let max_streams_bidi = sample_option_u32(ctx);
        let max_stream_bidi_remote = sample_option_u32(ctx);

        // いずれか一つ以上 Some にする
        let has_any = max_data.is_some()
            || max_stream_uni.is_some()
            || max_stream_bidi_local.is_some()
            || max_streams_uni.is_some()
            || max_streams_bidi.is_some()
            || max_stream_bidi_remote.is_some();

        let result = Limits::builder()
            .enable_connect_protocol(true)
            .wt_enabled(true)
            .webtransport(
                max_data,
                max_stream_uni,
                max_stream_bidi_local,
                max_streams_uni,
                max_streams_bidi,
                max_stream_bidi_remote,
            )
            .build();

        // WT 設定有無に関わらず enable_connect_protocol=true + wt_enabled=true なら成功する
        assert!(result.is_ok(), "has_any={has_any}");
        Ok(())
    })?;
    Ok(())
}

/// WebTransport 設定あり + enable_connect_protocol=false で build は失敗する
///
/// WT フィールドが 1 つでも設定されていれば、connect protocol が有効でない場合は
/// 失敗しなければならない。どのフィールドを設定するかを field_idx で選ぶ。
#[test]
fn prop_wt_without_connect_protocol_fails() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let wt_value = noprop::sample_u32(ctx);
        let field_idx = noprop::sample_usize_in(ctx, 0..6);
        let (mut max_data, mut max_stream_uni, mut max_stream_bidi_local) = (None, None, None);
        let (mut max_streams_uni, mut max_streams_bidi, mut max_stream_bidi_remote) =
            (None, None, None);

        match field_idx {
            0 => max_data = Some(wt_value),
            1 => max_stream_uni = Some(wt_value),
            2 => max_stream_bidi_local = Some(wt_value),
            3 => max_streams_uni = Some(wt_value),
            4 => max_streams_bidi = Some(wt_value),
            _ => max_stream_bidi_remote = Some(wt_value),
        }

        let result = Limits::builder()
            .enable_connect_protocol(false)
            .webtransport(
                max_data,
                max_stream_uni,
                max_stream_bidi_local,
                max_streams_uni,
                max_streams_bidi,
                max_stream_bidi_remote,
            )
            .build();
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// WebTransport 初期設定あり + wt_enabled=false + enable_connect_protocol=true で build は失敗する
///
/// draft-ietf-webtrans-http2-15 Section 3.1: WT 初期設定は SETTINGS_WT_ENABLED=1 の
/// サポート表明があって意味を持つ。
#[test]
fn prop_wt_initial_settings_without_wt_enabled_fails() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let wt_value = noprop::sample_u32(ctx);
        let field_idx = noprop::sample_usize_in(ctx, 0..6);
        let (mut max_data, mut max_stream_uni, mut max_stream_bidi_local) = (None, None, None);
        let (mut max_streams_uni, mut max_streams_bidi, mut max_stream_bidi_remote) =
            (None, None, None);

        match field_idx {
            0 => max_data = Some(wt_value),
            1 => max_stream_uni = Some(wt_value),
            2 => max_stream_bidi_local = Some(wt_value),
            3 => max_streams_uni = Some(wt_value),
            4 => max_streams_bidi = Some(wt_value),
            _ => max_stream_bidi_remote = Some(wt_value),
        }

        let result = Limits::builder()
            .enable_connect_protocol(true)
            .wt_enabled(false)
            .webtransport(
                max_data,
                max_stream_uni,
                max_stream_bidi_local,
                max_streams_uni,
                max_streams_bidi,
                max_stream_bidi_remote,
            )
            .build();
        assert!(result.is_err());
        assert_eq!(
            result.expect_err("should fail"),
            LimitsError::WebtransportRequiresWtEnabled
        );
        Ok(())
    })?;
    Ok(())
}

/// build したあとの getter が設定値と一致する
#[test]
fn prop_limits_getter_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_concurrent = sample_option_u32(ctx);
        let initial_window = WindowSize::from_static(noprop::sample_u64_in(
            ctx,
            0..=MAX_INITIAL_WINDOW_SIZE as u64,
        ) as u32);
        let max_frame = MaxFrameSize::from_static(noprop::sample_u64_in(
            ctx,
            MIN_MAX_FRAME_SIZE as u64..=MAX_MAX_FRAME_SIZE as u64,
        ) as u32);
        let max_header_list = sample_option_u32(ctx);
        let header_table = noprop::sample_u32(ctx);
        let connection_window = sample_valid_connection_window_size(ctx);

        let limits = Limits::builder()
            .max_concurrent_streams(max_concurrent)
            .initial_window_size(initial_window)
            .max_frame_size(max_frame)
            .max_header_list_size(max_header_list)
            .header_table_size(header_table)
            .connection_window_size(connection_window)
            .build()
            .expect("should succeed");

        assert_eq!(limits.max_concurrent_streams(), max_concurrent);
        assert_eq!(limits.initial_window_size(), initial_window);
        assert_eq!(limits.max_frame_size(), max_frame);
        assert_eq!(limits.max_header_list_size(), max_header_list);
        assert_eq!(limits.header_table_size(), header_table);
        assert_eq!(limits.connection_window_size(), connection_window);
        Ok(())
    })?;
    Ok(())
}
