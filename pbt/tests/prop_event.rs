//! HTTP/2 イベントの PBT
//!
//! Event 型のプロパティをテストする。

use shiguredo_http2::{ErrorCode, Event, HeaderField, StreamId};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// 有効なストリーム ID を生成する (1 以上、31 ビット範囲内)
fn sample_valid_stream_id(ctx: &mut noprop::TestCaseContext) -> StreamId {
    StreamId::from_wire(noprop::sample_u64_in(ctx, 1..=0x7FFF_FFFF) as u32)
}

/// ErrorCode を生成する
fn sample_error_code(ctx: &mut noprop::TestCaseContext) -> ErrorCode {
    match noprop::sample_weighted_index(ctx, &[1, 1, 1, 1, 1, 1]) {
        0 => ErrorCode::NoError,
        1 => ErrorCode::ProtocolError,
        2 => ErrorCode::InternalError,
        3 => ErrorCode::FlowControlError,
        4 => ErrorCode::Cancel,
        _ => ErrorCode::Unknown(noprop::sample_u32(ctx)),
    }
}

/// 小文字 ASCII または '-' の文字からなる name を生成する
fn sample_header_field_name(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz-";
    let len = noprop::sample_usize_in(ctx, 1..=20);
    (0..len)
        .map(|_| noprop::sample_choice(ctx, CHARSET))
        .collect()
}

/// 英数字の value を生成する
fn sample_header_field_value(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let len = noprop::sample_usize_in(ctx, 1..=50);
    (0..len)
        .map(|_| noprop::sample_choice(ctx, CHARSET))
        .collect()
}

/// HeaderField を生成する
fn sample_header_field(ctx: &mut noprop::TestCaseContext) -> HeaderField {
    let name = sample_header_field_name(ctx);
    let value = sample_header_field_value(ctx);
    HeaderField::new(&name, &value).expect("valid header field")
}

/// 0..=max_len バイトの任意バイト列を生成する
fn sample_arbitrary_bytes(ctx: &mut noprop::TestCaseContext, max_len: usize) -> Vec<u8> {
    let len = match max_len {
        0 => 0,
        1 => noprop::sample_with_boundaries(ctx, &[0usize, 1], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, 0..=1)
        }),
        _ => noprop::sample_with_boundaries(
            ctx,
            &[0usize, 1, max_len],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, 0..=max_len),
        ),
    };
    noprop::sample_bytes_vec(ctx, len)
}

/// ストリームレベルの Event を生成する
fn sample_stream_level_event(ctx: &mut noprop::TestCaseContext) -> Event {
    match noprop::sample_weighted_index(ctx, &[1; 7]) {
        0 => {
            let stream_id = sample_valid_stream_id(ctx);
            let header_count = noprop::sample_usize_in(ctx, 0..=5);
            let headers = (0..header_count)
                .map(|_| sample_header_field(ctx))
                .collect();
            let end_stream = noprop::sample_bool(ctx);
            // protocol は None か 1..=20 バイトの任意バイト列
            let protocol = if noprop::sample_bool(ctx) {
                Some(sample_arbitrary_bytes_min_1(ctx, 20))
            } else {
                None
            };
            Event::HeadersReceived {
                stream_id,
                headers,
                end_stream,
                protocol,
            }
        }
        1 => Event::DataReceived {
            stream_id: sample_valid_stream_id(ctx),
            data: sample_arbitrary_bytes(ctx, 100),
            end_stream: noprop::sample_bool(ctx),
        },
        2 => Event::TrailersReceived {
            stream_id: sample_valid_stream_id(ctx),
            trailers: {
                let count = noprop::sample_usize_in(ctx, 0..=5);
                (0..count).map(|_| sample_header_field(ctx)).collect()
            },
        },
        3 => Event::StreamReset {
            stream_id: sample_valid_stream_id(ctx),
            error_code: sample_error_code(ctx),
            // 受信パス・公開 API は常に 0 だが、接続ウィンドウ消費量の範囲で 0 も含めて検証する
            connection_window_consumed: noprop::sample_usize_in(ctx, 0..=1_048_576),
        },
        4 => Event::DataDiscarded {
            stream_id: sample_valid_stream_id(ctx),
            // 実装は flow_control_size > 0 のときのみ生成するため 0 は含めない
            connection_window_consumed: 1 + noprop::sample_usize_in(ctx, 0..=1_048_576),
        },
        5 => Event::StreamClosed {
            stream_id: sample_valid_stream_id(ctx),
        },
        _ => Event::WindowUpdateReceived {
            stream_id: sample_valid_stream_id(ctx),
            increment: noprop::sample_u64_in(ctx, 1..=u32::MAX as u64) as u32,
        },
    }
}

/// 1..=max_len バイトの任意バイト列を生成する
fn sample_arbitrary_bytes_min_1(ctx: &mut noprop::TestCaseContext, max_len: usize) -> Vec<u8> {
    let len = 1 + noprop::sample_usize_in(ctx, 0..max_len);
    noprop::sample_bytes_vec(ctx, len)
}

/// 接続レベルの Event を生成する
fn sample_connection_level_event(ctx: &mut noprop::TestCaseContext) -> Event {
    match noprop::sample_weighted_index(ctx, &[1; 7]) {
        0 => Event::ConnectionPreface,
        1 => Event::SettingsReceived {
            ack: noprop::sample_bool(ctx),
        },
        2 => Event::PingReceived {
            opaque_data: noprop::sample_bytes::<8>(ctx),
            ack: noprop::sample_bool(ctx),
        },
        3 => Event::GoawayReceived {
            last_stream_id: StreamId::from_wire(noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFF) as u32),
            error_code: sample_error_code(ctx),
            debug_data: sample_arbitrary_bytes(ctx, 50),
        },
        4 => Event::WindowUpdateReceived {
            stream_id: StreamId::Connection,
            increment: noprop::sample_u64_in(ctx, 1..=u32::MAX as u64) as u32,
        },
        _ => Event::ConnectionError {
            error_code: sample_error_code(ctx),
            reason: {
                const CHARSET: &[u8] =
                    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";
                let len = noprop::sample_usize_in(ctx, 0..=50);
                (0..len)
                    .map(|_| noprop::sample_choice(ctx, CHARSET) as char)
                    .collect()
            },
        },
    }
}

/// ストリームレベルイベントは stream_id() が Some を返す
#[test]
fn prop_stream_level_event_has_stream_id() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let event = sample_stream_level_event(ctx);
        assert!(
            event.stream_id().is_some(),
            "ストリームレベルイベントは stream_id() が Some を返す: {event:?}"
        );
        Ok(())
    })?;
    Ok(())
}

/// ストリームレベルイベントは is_connection_level() が false を返す
#[test]
fn prop_stream_level_event_not_connection_level() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let event = sample_stream_level_event(ctx);
        assert!(!event.is_connection_level());
        Ok(())
    })?;
    Ok(())
}

/// 接続レベルイベントは is_connection_level() が true を返す
#[test]
fn prop_connection_level_event_is_connection_level() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let event = sample_connection_level_event(ctx);
        assert!(event.is_connection_level());
        Ok(())
    })?;
    Ok(())
}

/// 接続レベルイベントは stream_id() が None を返す
#[test]
fn prop_connection_level_event_has_no_stream_id() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let event = sample_connection_level_event(ctx);
        assert!(
            event.stream_id().is_none(),
            "接続レベルイベントは stream_id() が None を返す: {event:?}"
        );
        Ok(())
    })?;
    Ok(())
}

/// WindowUpdateReceived の stream_id による分類
///
/// stream_id == 0 なら接続レベル、それ以外はストリームレベル
#[test]
fn prop_window_update_classification() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = StreamId::from_wire(noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFF) as u32);
        let increment = 1 + noprop::sample_u64_in(ctx, 0..u32::MAX as u64) as u32;
        let event = Event::WindowUpdateReceived {
            stream_id,
            increment,
        };

        if matches!(stream_id, StreamId::Connection) {
            assert!(event.is_connection_level());
            assert!(event.stream_id().is_none());
        } else {
            assert!(!event.is_connection_level());
            assert_eq!(event.stream_id(), Some(stream_id));
        }
        Ok(())
    })?;
    Ok(())
}

/// stream_id() が返す値は実際のストリーム ID と一致する
#[test]
fn prop_stream_id_value_matches() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let data = sample_arbitrary_bytes(ctx, 10);
        let events = vec![
            Event::HeadersReceived {
                stream_id,
                headers: vec![],
                end_stream: false,
                protocol: None,
            },
            Event::DataReceived {
                stream_id,
                data,
                end_stream: false,
            },
            Event::TrailersReceived {
                stream_id,
                trailers: vec![],
            },
            Event::StreamReset {
                stream_id,
                error_code: ErrorCode::NoError,
                connection_window_consumed: 0,
            },
            // stream_id() の返却値の検証のみが目的の固定サンプルであり、
            // connection_window_consumed は 0 でも実害はない (ストリームレベルの流れを検証している)
            Event::DataDiscarded {
                stream_id,
                connection_window_consumed: 0,
            },
            Event::StreamClosed { stream_id },
            Event::WindowUpdateReceived {
                stream_id,
                increment: 1000,
            },
            Event::PriorityUpdateReceived {
                stream_id,
                priority_field_value: vec![],
            },
        ];

        for event in events {
            assert_eq!(event.stream_id(), Some(stream_id));
        }
        Ok(())
    })?;
    Ok(())
}
