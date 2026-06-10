//! Limits ビルダーの PBT
//!
//! LimitsBuilder::build() の複合制約検査を検証する。

use proptest::prelude::*;
use shiguredo_http2::settings::{
    DEFAULT_INITIAL_WINDOW_SIZE, MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, MIN_MAX_FRAME_SIZE,
};
use shiguredo_http2::{Limits, MaxFrameSize, WindowSize};

fn valid_window_size() -> impl Strategy<Value = WindowSize> {
    (0u32..=MAX_INITIAL_WINDOW_SIZE).prop_map(WindowSize::from_static)
}

/// 接続レベルの有効なウィンドウサイズを生成する
///
/// RFC 9113 Section 6.9.2: 接続レベルのウィンドウは SETTINGS では縮小できないため、
/// `LimitsBuilder::connection_window_size` は `DEFAULT_INITIAL_WINDOW_SIZE` 未満を拒否する。
fn valid_connection_window_size() -> impl Strategy<Value = WindowSize> {
    (DEFAULT_INITIAL_WINDOW_SIZE..=MAX_INITIAL_WINDOW_SIZE).prop_map(WindowSize::from_static)
}

fn valid_max_frame_size() -> impl Strategy<Value = MaxFrameSize> {
    (MIN_MAX_FRAME_SIZE..=MAX_MAX_FRAME_SIZE).prop_map(MaxFrameSize::from_static)
}

proptest! {
    /// 有効な LimitsBuilder 設定で build は常に成功する (WT なし)
    #[test]
    fn prop_valid_limits_build_succeeds(
        max_concurrent in prop::option::of(any::<u32>()),
        initial_window in valid_window_size(),
        max_frame in valid_max_frame_size(),
        max_header_list in prop::option::of(any::<u32>()),
        header_table in any::<u32>(),
        connection_window in valid_connection_window_size(),
        enable_connect in prop::bool::ANY,
        no_rfc7540 in prop::bool::ANY,
    ) {
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
        prop_assert!(result.is_ok());
    }

    /// WebTransport 設定 + enable_connect_protocol=true で build は成功する
    #[test]
    fn prop_wt_with_connect_protocol_succeeds(
        max_data in prop::option::of(any::<u32>()),
        max_stream_uni in prop::option::of(any::<u32>()),
        max_stream_bidi_local in prop::option::of(any::<u32>()),
        max_streams_uni in prop::option::of(any::<u32>()),
        max_streams_bidi in prop::option::of(any::<u32>()),
        max_stream_bidi_remote in prop::option::of(any::<u32>()),
    ) {
        // いずれか一つ以上 Some にする
        let has_any = max_data.is_some()
            || max_stream_uni.is_some()
            || max_stream_bidi_local.is_some()
            || max_streams_uni.is_some()
            || max_streams_bidi.is_some()
            || max_stream_bidi_remote.is_some();

        let result = Limits::builder()
            .enable_connect_protocol(true)
            .webtransport(
                max_data,
                max_stream_uni,
                max_stream_bidi_local,
                max_streams_uni,
                max_streams_bidi,
                max_stream_bidi_remote,
            )
            .build();

        // WT 設定有無に関わらず enable_connect_protocol=true なら成功する
        prop_assert!(result.is_ok(), "has_any={has_any}");
    }

    /// WebTransport 設定あり + enable_connect_protocol=false で build は失敗する
    #[test]
    fn prop_wt_without_connect_protocol_fails(
        wt_value in any::<u32>(),
        field_idx in 0usize..6,
    ) {
        let mut max_data = None;
        let mut max_stream_uni = None;
        let mut max_stream_bidi_local = None;
        let mut max_streams_uni = None;
        let mut max_streams_bidi = None;
        let mut max_stream_bidi_remote = None;

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
        prop_assert!(result.is_err());
    }

    /// build したあとの getter が設定値と一致する
    #[test]
    fn prop_limits_getter_roundtrip(
        max_concurrent in prop::option::of(any::<u32>()),
        initial_window in valid_window_size(),
        max_frame in valid_max_frame_size(),
        max_header_list in prop::option::of(any::<u32>()),
        header_table in any::<u32>(),
        connection_window in valid_connection_window_size(),
    ) {
        let limits = Limits::builder()
            .max_concurrent_streams(max_concurrent)
            .initial_window_size(initial_window)
            .max_frame_size(max_frame)
            .max_header_list_size(max_header_list)
            .header_table_size(header_table)
            .connection_window_size(connection_window)
            .build()
            .unwrap();

        prop_assert_eq!(limits.max_concurrent_streams(), max_concurrent);
        prop_assert_eq!(limits.initial_window_size(), initial_window);
        prop_assert_eq!(limits.max_frame_size(), max_frame);
        prop_assert_eq!(limits.max_header_list_size(), max_header_list);
        prop_assert_eq!(limits.header_table_size(), header_table);
        prop_assert_eq!(limits.connection_window_size(), connection_window);
    }
}
