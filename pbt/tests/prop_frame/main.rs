//! フレームエンコード/デコードの PBT
//!
//! RFC 9113 Section 4, 6 に基づくフレームのエンコード/デコードを検証する。

use shiguredo_http2::frame::{ContinuationFrame, FrameFlags, FrameHeader, PriorityUpdateFrame};
use shiguredo_http2::settings::{MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, MIN_MAX_FRAME_SIZE};
use shiguredo_http2::{
    DataFrame, Frame, FrameDecoder, FrameEncoder, GoawayFrame, HeadersFrame, LastStreamId,
    MaxFrameSize, NonZeroStreamId, PingFrame, RstStreamFrame, Setting, SettingsFrame, StreamId,
    Weight, WindowIncrement, WindowSize, WindowUpdateFrame,
};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// 有効なストリーム ID (0 以外) を生成する
fn sample_valid_stream_id(ctx: &mut noprop::TestCaseContext) -> NonZeroStreamId {
    let id = noprop::sample_with_boundaries(
        ctx,
        &[1u32, 0x7FFF_FFFF],
        noprop::Ratio::one_nth(5),
        |ctx| 1 + noprop::sample_u64_in(ctx, 0..0x7FFF_FFFFu64) as u32,
    );
    NonZeroStreamId::new(id).expect("valid non-zero stream ID")
}

/// 有効な Setting を生成する
fn sample_valid_setting(ctx: &mut noprop::TestCaseContext) -> Setting {
    match noprop::sample_weighted_index(ctx, &[1; 15]) {
        0 => Setting::HeaderTableSize(noprop::sample_u32(ctx)),
        1 => Setting::EnablePush(noprop::sample_bool(ctx)),
        2 => Setting::MaxConcurrentStreams(noprop::sample_u32(ctx)),
        3 => Setting::InitialWindowSize(
            WindowSize::new(noprop::sample_u64_in(ctx, 0..=MAX_INITIAL_WINDOW_SIZE as u64) as u32)
                .expect("valid SETTINGS value"),
        ),
        4 => Setting::MaxFrameSize(
            MaxFrameSize::new(noprop::sample_u64_in(
                ctx,
                MIN_MAX_FRAME_SIZE as u64..=MAX_MAX_FRAME_SIZE as u64,
            ) as u32)
            .expect("valid SETTINGS value"),
        ),
        5 => Setting::MaxHeaderListSize(noprop::sample_u32(ctx)),
        6 => Setting::EnableConnectProtocol(noprop::sample_bool(ctx)),
        7 => Setting::NoRfc7540Priorities(noprop::sample_bool(ctx)),
        8 => Setting::WtInitialMaxData(noprop::sample_u32(ctx)),
        9 => Setting::WtInitialMaxStreamDataUni(noprop::sample_u32(ctx)),
        10 => Setting::WtInitialMaxStreamDataBidiLocal(noprop::sample_u32(ctx)),
        11 => Setting::WtInitialMaxStreamsUni(noprop::sample_u32(ctx)),
        12 => Setting::WtInitialMaxStreamsBidi(noprop::sample_u32(ctx)),
        13 => Setting::WtInitialMaxStreamDataBidiRemote(noprop::sample_u32(ctx)),
        _ => Setting::WtEnabled(noprop::sample_bool(ctx)),
    }
}

/// 任意のバイト列 (0..=max_len) を生成する
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

/// エンコード→デコードの往復と一致を検証する共通補助関数
///
/// エンコードしたバイト列を 1 フレームとしてデコードし、`check` でフレーム内容を検証する。
fn roundtrip<F>(frame: &Frame, check: F)
where
    F: FnOnce(Frame),
{
    let mut encoder = FrameEncoder::new();
    encoder.encode(frame).expect("encode should succeed");
    let encoded = encoder.take();

    let mut decoder = FrameDecoder::new(16384);
    decoder.feed(&encoded);
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("parse should succeed");
    check(decoded);
}

/// DATA フレームのエンコード/デコード往復テスト
#[test]
fn prop_data_frame_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let end_stream = noprop::sample_bool(ctx);
        let data = sample_arbitrary_bytes(ctx, 1024);
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream,
            data: data.clone(),
            pad_length: None,
        });

        roundtrip(&frame, |decoded| {
            if let Frame::Data(df) = decoded {
                assert_eq!(df.stream_id, stream_id);
                assert_eq!(df.end_stream, end_stream);
                assert_eq!(df.data, data);
            } else {
                panic!("expected DATA frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// HEADERS フレームのエンコード/デコード往復テスト
#[test]
fn prop_headers_frame_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let end_stream = noprop::sample_bool(ctx);
        let end_headers = noprop::sample_bool(ctx);
        let header_block = sample_arbitrary_bytes(ctx, 512);
        let frame = Frame::Headers(HeadersFrame {
            stream_id,
            end_stream,
            end_headers,
            priority_fields: None,
            header_block_fragment: header_block.clone(),
            pad_length: None,
        });

        roundtrip(&frame, |decoded| {
            if let Frame::Headers(hf) = decoded {
                assert_eq!(hf.stream_id, stream_id);
                assert_eq!(hf.end_stream, end_stream);
                assert_eq!(hf.end_headers, end_headers);
                assert_eq!(hf.header_block_fragment, header_block);
            } else {
                panic!("expected HEADERS frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// RST_STREAM フレームのエンコード/デコード往復テスト
#[test]
fn prop_rst_stream_frame_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let error_code = noprop::sample_u32(ctx);
        let frame = Frame::RstStream(RstStreamFrame {
            stream_id,
            error_code,
        });

        roundtrip(&frame, |decoded| {
            if let Frame::RstStream(rf) = decoded {
                assert_eq!(rf.stream_id, stream_id);
                assert_eq!(rf.error_code, error_code);
            } else {
                panic!("expected RST_STREAM frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// PING フレームのエンコード/デコード往復テスト
#[test]
fn prop_ping_frame_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let ack = noprop::sample_bool(ctx);
        let opaque_data = noprop::sample_bytes::<8>(ctx);
        let frame = Frame::Ping(PingFrame { ack, opaque_data });

        roundtrip(&frame, |decoded| {
            if let Frame::Ping(pf) = decoded {
                assert_eq!(pf.ack, ack);
                assert_eq!(pf.opaque_data, opaque_data);
            } else {
                panic!("expected PING frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// GOAWAY フレームのエンコード/デコード往復テスト
#[test]
fn prop_goaway_frame_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let last_stream_id =
            LastStreamId::new(noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64) as u32)
                .expect("valid last stream ID");
        let error_code = noprop::sample_u32(ctx);
        let debug_data = sample_arbitrary_bytes(ctx, 128);
        let frame = Frame::Goaway(GoawayFrame {
            last_stream_id,
            error_code,
            debug_data: debug_data.clone(),
        });

        roundtrip(&frame, |decoded| {
            if let Frame::Goaway(gf) = decoded {
                assert_eq!(gf.last_stream_id, last_stream_id);
                assert_eq!(gf.error_code, error_code);
                assert_eq!(gf.debug_data, debug_data);
            } else {
                panic!("expected GOAWAY frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// WINDOW_UPDATE フレームのエンコード/デコード往復テスト（接続レベル）
#[test]
fn prop_window_update_frame_connection_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // WINDOW_UPDATE の increment は 1 以上でなければならない
        let increment_raw = 1 + noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64 - 1) as u32;
        let increment = WindowIncrement::new(increment_raw).expect("valid window increment");
        let frame = Frame::WindowUpdate(WindowUpdateFrame::for_connection(increment));

        roundtrip(&frame, |decoded| {
            if let Frame::WindowUpdate(wuf) = decoded {
                assert_eq!(wuf.stream_id, StreamId::Connection);
                assert_eq!(wuf.window_size_increment, increment);
            } else {
                panic!("expected WINDOW_UPDATE frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// WINDOW_UPDATE フレームのエンコード/デコード往復テスト（ストリームレベル）
#[test]
fn prop_window_update_frame_stream_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        // WINDOW_UPDATE の increment は 1 以上でなければならない
        let increment_raw = 1 + noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64 - 1) as u32;
        let increment = WindowIncrement::new(increment_raw).expect("valid window increment");
        let frame = Frame::WindowUpdate(WindowUpdateFrame::for_stream(stream_id, increment));

        roundtrip(&frame, |decoded| {
            if let Frame::WindowUpdate(wuf) = decoded {
                assert_eq!(wuf.stream_id, StreamId::from(stream_id));
                assert_eq!(wuf.window_size_increment, increment);
            } else {
                panic!("expected WINDOW_UPDATE frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// CONTINUATION フレームのエンコード/デコード往復テスト
#[test]
fn prop_continuation_frame_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let end_headers = noprop::sample_bool(ctx);
        let header_block = sample_arbitrary_bytes(ctx, 512);
        let frame = Frame::Continuation(ContinuationFrame {
            stream_id,
            end_headers,
            header_block_fragment: header_block.clone(),
        });

        roundtrip(&frame, |decoded| {
            if let Frame::Continuation(cf) = decoded {
                assert_eq!(cf.stream_id, stream_id);
                assert_eq!(cf.end_headers, end_headers);
                assert_eq!(cf.header_block_fragment, header_block);
            } else {
                panic!("expected CONTINUATION frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// PRIORITY_UPDATE フレームのエンコード/デコード往復テスト
#[test]
fn prop_priority_update_frame_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let prioritized_element_id =
            NonZeroStreamId::new(1 + noprop::sample_u64_in(ctx, 0..0x7FFF_FFFFu64) as u32)
                .expect("valid non-zero stream ID");
        let priority_field_value = sample_arbitrary_bytes(ctx, 128);
        let frame = Frame::PriorityUpdate(PriorityUpdateFrame {
            prioritized_element_id,
            priority_field_value: priority_field_value.clone(),
        });

        roundtrip(&frame, |decoded| {
            if let Frame::PriorityUpdate(puf) = decoded {
                assert_eq!(puf.prioritized_element_id, prioritized_element_id);
                assert_eq!(puf.priority_field_value, priority_field_value);
            } else {
                panic!("expected PRIORITY_UPDATE frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// Unknown フレームのエンコード/デコード往復テスト
///
/// RFC 9113 Section 4.1: 未知のフレームタイプは無視し破棄しなければならない (MUST)
#[test]
fn prop_unknown_frame_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 未知のフレームタイプ (0x0a-0x0f, 0x11-0xff)
        // 注: 0x05 (PUSH_PROMISE) は既知のフレームタイプとして処理される
        let frame_type = match noprop::sample_usize_in(ctx, 0..2) {
            0 => noprop::sample_u64_in(ctx, 0x0a..=0x0f) as u8,
            _ => noprop::sample_u64_in(ctx, 0x11..=0xff) as u8,
        };
        let stream_id = noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64) as u32;
        let payload = sample_arbitrary_bytes(ctx, 128);
        let header = FrameHeader {
            length: payload.len() as u32,
            frame_type,
            flags: FrameFlags::empty(),
            stream_id,
        };

        let frame = Frame::Unknown {
            header,
            payload: payload.clone(),
        };

        roundtrip(&frame, |decoded| {
            if let Frame::Unknown {
                header: h,
                payload: p,
            } = decoded
            {
                assert_eq!(h.frame_type, frame_type);
                assert_eq!(h.stream_id, stream_id);
                assert_eq!(p, payload);
            } else {
                panic!("expected Unknown frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// パディング付き DATA フレームのエンコード/デコード往復テスト
#[test]
fn prop_data_frame_with_padding_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let end_stream = noprop::sample_bool(ctx);
        let data = sample_arbitrary_bytes(ctx, 512);
        let pad_length = noprop::sample_u64_in(ctx, 0..=128) as u8;
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream,
            data: data.clone(),
            pad_length: Some(pad_length),
        });

        roundtrip(&frame, |decoded| {
            if let Frame::Data(df) = decoded {
                assert_eq!(df.stream_id, stream_id);
                assert_eq!(df.end_stream, end_stream);
                assert_eq!(df.data, data);
                assert_eq!(df.pad_length, Some(pad_length));
            } else {
                panic!("expected DATA frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// パディング付き HEADERS フレームのエンコード/デコード往復テスト
#[test]
fn prop_headers_frame_with_padding_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let end_stream = noprop::sample_bool(ctx);
        let end_headers = noprop::sample_bool(ctx);
        let header_block = sample_arbitrary_bytes(ctx, 256);
        let pad_length = noprop::sample_u64_in(ctx, 0..=64) as u8;
        let frame = Frame::Headers(HeadersFrame {
            stream_id,
            end_stream,
            end_headers,
            priority_fields: None,
            header_block_fragment: header_block.clone(),
            pad_length: Some(pad_length),
        });

        roundtrip(&frame, |decoded| {
            if let Frame::Headers(hf) = decoded {
                assert_eq!(hf.stream_id, stream_id);
                assert_eq!(hf.end_stream, end_stream);
                assert_eq!(hf.end_headers, end_headers);
                assert_eq!(hf.header_block_fragment, header_block);
                assert_eq!(hf.pad_length, Some(pad_length));
            } else {
                panic!("expected HEADERS frame");
            }
        });
        Ok(())
    })?;
    Ok(())
}

/// 複数フレームの連続デコード
///
/// 数学的意義: デコーダーの状態遷移の正当性
#[test]
fn prop_multiple_frames_decode() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let frame_count = noprop::sample_usize_in(ctx, 2..=9);
        let stream_ids: Vec<NonZeroStreamId> = (0..frame_count)
            .map(|_| sample_valid_stream_id(ctx))
            .collect();
        let mut encoder = FrameEncoder::new();

        // 複数フレームをエンコード
        let increment = WindowIncrement::new(1000).expect("valid window increment");
        let frames: Vec<Frame> = stream_ids
            .iter()
            .take(frame_count)
            .map(|sid| Frame::WindowUpdate(WindowUpdateFrame::for_stream(*sid, increment)))
            .collect();

        for frame in &frames {
            encoder.encode(frame).expect("encode should succeed");
        }
        let encoded = encoder.take();

        // 連続デコード
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);

        for (i, original) in frames.iter().enumerate() {
            let decoded = decoder.decode().expect("decode should succeed");
            assert!(decoded.is_some(), "Frame {} should be decoded", i);
            assert_eq!(
                decoded.expect("should succeed").stream_id(),
                original.stream_id(),
                "Frame {} stream_id mismatch",
                i
            );
        }

        // これ以上フレームがないことを確認
        assert!(decoder.decode().expect("decode should succeed").is_none());
        Ok(())
    })?;
    Ok(())
}

/// 部分的データの feed でのデコード
///
/// 数学的意義: ストリーミングデコードの正当性
#[test]
fn prop_partial_feed_decode() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let data = sample_arbitrary_bytes(ctx, 100);
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data: data.clone(),
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);

        // 1 バイトずつ feed
        for (i, byte) in encoded.iter().enumerate() {
            decoder.feed(&[*byte]);

            // 最後のバイトまではデコードできない
            if i < encoded.len() - 1 {
                let result = decoder.decode().expect("decode should succeed");
                assert!(
                    result.is_none(),
                    "Should not decode until all bytes are fed (at byte {})",
                    i
                );
            }
        }

        // 全バイト feed 後はデコードできる
        let decoded = decoder
            .decode()
            .expect("decode should succeed")
            .expect("decode should succeed");
        assert_eq!(decoded.stream_id(), StreamId::from(stream_id));
        Ok(())
    })?;
    Ok(())
}

/// SETTINGS フレームの設定値エンコード/デコード
#[test]
fn prop_settings_frame_values_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let count = noprop::sample_usize_in(ctx, 1..=9);
        let settings: Vec<Setting> = (0..count).map(|_| sample_valid_setting(ctx)).collect();
        let frame = Frame::Settings(SettingsFrame::from_settings(settings.clone()));

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");

        if let Frame::Settings(sf) = decoded {
            assert!(!sf.is_ack());
            assert_eq!(sf.settings().len(), settings.len());
            for (orig, decoded) in settings.iter().zip(sf.settings().iter()) {
                assert_eq!(orig.as_wire(), decoded.as_wire());
            }
        } else {
            panic!("expected SETTINGS frame");
        }
        Ok(())
    })?;
    Ok(())
}

/// フレームサイズ超過の検出
///
/// 数学的意義: max_frame_size 制約の検証
#[test]
fn prop_frame_size_exceeded_detected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_frame_size = noprop::sample_u64_in(ctx, 100..=999) as u32;
        let data_len = noprop::sample_usize_in(ctx, 1000..=2000);
        // max_frame_size (100..=999) < data_len (1000..=2000) は常に成立する
        assert!(data_len > max_frame_size as usize);

        let frame = Frame::Data(DataFrame {
            stream_id: NonZeroStreamId::from_static(1),
            end_stream: false,
            data: vec![0u8; data_len],
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(max_frame_size);
        decoder.feed(&encoded);

        // デコード時にエラーが発生する
        let result = decoder.decode();
        assert!(
            result.is_err(),
            "Should reject frame exceeding max_frame_size"
        );
        Ok(())
    })?;
    Ok(())
}

/// エンコードされたフレームヘッダーは 9 バイト
///
/// 数学的意義: フレームヘッダーサイズの不変条件
#[test]
fn prop_frame_header_size_invariant() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let data = sample_arbitrary_bytes(ctx, 100);
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data: data.clone(),
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        // フレームヘッダー (9 バイト) + ペイロード
        assert_eq!(encoded.len(), 9 + data.len());
        Ok(())
    })?;
    Ok(())
}

/// ストリーム ID の上位ビットは予約済み
///
/// RFC 9113 Section 4.1: R ビットは予約済み (0)
#[test]
fn prop_stream_id_reserved_bit() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = StreamId::from_wire(noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64) as u32);
        let increment = WindowIncrement::new(1000).expect("valid window increment");
        let frame = match stream_id.non_zero() {
            Some(nz) => Frame::WindowUpdate(WindowUpdateFrame::for_stream(nz, increment)),
            None => Frame::WindowUpdate(WindowUpdateFrame::for_connection(increment)),
        };

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        // フレームヘッダーの 5 バイト目 (オフセット 5) の上位ビットは 0
        assert_eq!(encoded[5] & 0x80, 0, "Reserved bit must be 0");

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");

        assert_eq!(decoded.stream_id(), stream_id);
        Ok(())
    })?;
    Ok(())
}

/// SETTINGS フレームの stream ID != 0 はエラー (RFC 9113 Section 6.5: SETTINGS の stream ID は 0 でなければならない)
#[test]
fn prop_settings_non_zero_stream_id_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 0]); // length = 0 (ACK)
        buf.push(0x04); // SETTINGS frame type
        buf.push(0x01); // ACK flag
        // stream_id (非ゼロ)
        buf.push(((sid >> 24) & 0x7f) as u8);
        buf.push(((sid >> 16) & 0xff) as u8);
        buf.push(((sid >> 8) & 0xff) as u8);
        buf.push((sid & 0xff) as u8);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "SETTINGS frame with non-zero stream_id should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// PING フレームの stream ID != 0 はエラー (RFC 9113 Section 6.7)
#[test]
fn prop_ping_non_zero_stream_id_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let opaque_data = noprop::sample_bytes::<8>(ctx);
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 8]); // length = 8
        buf.push(0x06); // PING frame type
        buf.push(0x00); // flags
        // stream_id (非ゼロ)
        buf.push(((sid >> 24) & 0x7f) as u8);
        buf.push(((sid >> 16) & 0xff) as u8);
        buf.push(((sid >> 8) & 0xff) as u8);
        buf.push((sid & 0xff) as u8);
        buf.extend_from_slice(&opaque_data);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "PING frame with non-zero stream_id should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// GOAWAY フレームの stream ID != 0 はエラー (RFC 9113 Section 6.8)
#[test]
fn prop_goaway_non_zero_stream_id_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let last_stream_id = noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64) as u32;
        let error_code = noprop::sample_u32(ctx);
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 8]); // length = 8
        buf.push(0x07); // GOAWAY frame type
        buf.push(0x00); // flags
        // stream_id (非ゼロ)
        buf.push(((sid >> 24) & 0x7f) as u8);
        buf.push(((sid >> 16) & 0xff) as u8);
        buf.push(((sid >> 8) & 0xff) as u8);
        buf.push((sid & 0xff) as u8);
        // last_stream_id
        buf.push(((last_stream_id >> 24) & 0x7f) as u8);
        buf.push(((last_stream_id >> 16) & 0xff) as u8);
        buf.push(((last_stream_id >> 8) & 0xff) as u8);
        buf.push((last_stream_id & 0xff) as u8);
        buf.extend_from_slice(&error_code.to_be_bytes());

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "GOAWAY frame with non-zero stream_id should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// PRIORITY_UPDATE フレームの stream ID != 0 はエラー (RFC 9218 Section 7.1: PRIORITY_UPDATE の Stream Identifier は 0 でなければならない)
#[test]
fn prop_priority_update_non_zero_stream_id_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let prioritized_element_id = noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64) as u32;
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 4]); // length = 4
        buf.push(0x10); // PRIORITY_UPDATE frame type
        buf.push(0x00); // flags
        // stream_id (非ゼロ)
        buf.push(((sid >> 24) & 0x7f) as u8);
        buf.push(((sid >> 16) & 0xff) as u8);
        buf.push(((sid >> 8) & 0xff) as u8);
        buf.push((sid & 0xff) as u8);
        // prioritized_element_id
        buf.push(((prioritized_element_id >> 24) & 0x7f) as u8);
        buf.push(((prioritized_element_id >> 16) & 0xff) as u8);
        buf.push(((prioritized_element_id >> 8) & 0xff) as u8);
        buf.push((prioritized_element_id & 0xff) as u8);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "PRIORITY_UPDATE frame with non-zero stream_id should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// SETTINGS ACK で空でないペイロードはエラー
///
/// RFC 9113 Section 6.5: ACK with non-empty payload is an error
#[test]
fn prop_settings_ack_non_empty_payload_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let payload_len = noprop::sample_usize_in(ctx, 1..=99);
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x04); // SETTINGS frame type
        buf.push(0x01); // ACK flag
        buf.extend_from_slice(&[0, 0, 0, 0]); // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "SETTINGS ACK with non-empty payload should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// SETTINGS ペイロードが 6 の倍数でないとエラー
///
/// RFC 9113 Section 6.5: payload must be multiple of 6
#[test]
fn prop_settings_payload_not_multiple_of_6_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 6 の倍数でない長さ
        let extra_bytes = noprop::sample_usize_in(ctx, 1..=4);
        let setting_count = noprop::sample_usize_in(ctx, 0..=4);
        let payload_len = setting_count * 6 + extra_bytes;
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x04); // SETTINGS frame type
        buf.push(0x00); // no ACK flag
        buf.extend_from_slice(&[0, 0, 0, 0]); // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "SETTINGS payload not multiple of 6 should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// RST_STREAM ペイロードが 4 バイトでないとエラー
///
/// RFC 9113 Section 6.4: RST_STREAM must be exactly 4 bytes
#[test]
fn prop_rst_stream_wrong_size_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let payload_len = match noprop::sample_usize_in(ctx, 0..2) {
            0 => noprop::sample_usize_in(ctx, 0..4),
            _ => 5 + noprop::sample_usize_in(ctx, 0..15),
        };
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x03); // RST_STREAM frame type
        buf.push(0x00); // flags
        buf.extend_from_slice(&[0, 0, 0, 1]); // stream_id = 1
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(result.is_err(), "RST_STREAM with wrong size should error");
        Ok(())
    })?;
    Ok(())
}

/// PING ペイロードが 8 バイトでないとエラー
///
/// RFC 9113 Section 6.7: PING must be exactly 8 bytes
#[test]
fn prop_ping_wrong_size_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let payload_len = match noprop::sample_usize_in(ctx, 0..2) {
            0 => noprop::sample_usize_in(ctx, 0..8),
            _ => 9 + noprop::sample_usize_in(ctx, 0..11),
        };
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x06); // PING frame type
        buf.push(0x00); // flags
        buf.extend_from_slice(&[0, 0, 0, 0]); // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(result.is_err(), "PING with wrong size should error");
        Ok(())
    })?;
    Ok(())
}

/// GOAWAY ペイロードが 8 バイト未満はエラー
///
/// RFC 9113 Section 6.8: GOAWAY must be at least 8 bytes
#[test]
fn prop_goaway_too_short_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let payload_len = noprop::sample_usize_in(ctx, 0..8);
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x07); // GOAWAY frame type
        buf.push(0x00); // flags
        buf.extend_from_slice(&[0, 0, 0, 0]); // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "GOAWAY with less than 8 bytes should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// WINDOW_UPDATE ペイロードが 4 バイトでないとエラー
///
/// RFC 9113 Section 6.9: WINDOW_UPDATE must be exactly 4 bytes
#[test]
fn prop_window_update_wrong_size_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let payload_len = match noprop::sample_usize_in(ctx, 0..2) {
            0 => noprop::sample_usize_in(ctx, 0..4),
            _ => 5 + noprop::sample_usize_in(ctx, 0..15),
        };
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x08); // WINDOW_UPDATE frame type
        buf.push(0x00); // flags
        buf.extend_from_slice(&[0, 0, 0, 0]); // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "WINDOW_UPDATE with wrong size should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// PRIORITY_UPDATE ペイロードが 4 バイト未満はエラー
///
/// RFC 9218 Section 7.1: PRIORITY_UPDATE must be at least 4 bytes
#[test]
fn prop_priority_update_too_short_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let payload_len = noprop::sample_usize_in(ctx, 0..4);
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x10); // PRIORITY_UPDATE frame type
        buf.push(0x00); // flags
        buf.extend_from_slice(&[0, 0, 0, 0]); // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(
            result.is_err(),
            "PRIORITY_UPDATE with less than 4 bytes should error"
        );
        Ok(())
    })?;
    Ok(())
}

/// PRIORITY フレームのデコードテスト (非推奨、受信は処理する)
#[test]
fn prop_priority_frame_decode() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id_raw = 1 + noprop::sample_u64_in(ctx, 0..0x7FFF_FFFFu64) as u32;
        let stream_dependency_raw = noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64) as u32;
        let weight = noprop::sample_u8(ctx);
        let exclusive = noprop::sample_bool(ctx);

        // 手動で PRIORITY フレームを構築
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 5]); // length = 5
        buf.push(0x02); // PRIORITY frame type
        buf.push(0x00); // flags
        // stream_id
        buf.push(((stream_id_raw >> 24) & 0x7f) as u8);
        buf.push(((stream_id_raw >> 16) & 0xff) as u8);
        buf.push(((stream_id_raw >> 8) & 0xff) as u8);
        buf.push((stream_id_raw & 0xff) as u8);
        // payload: exclusive + stream_dependency + weight
        let e_bit = if exclusive { 0x80 } else { 0x00 };
        buf.push(((stream_dependency_raw >> 24) & 0x7f) as u8 | e_bit);
        buf.push(((stream_dependency_raw >> 16) & 0xff) as u8);
        buf.push(((stream_dependency_raw >> 8) & 0xff) as u8);
        buf.push((stream_dependency_raw & 0xff) as u8);
        buf.push(weight);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");

        if let Frame::Priority(pf) = result {
            let expected_stream_id =
                NonZeroStreamId::new(stream_id_raw).expect("valid non-zero stream ID");
            assert_eq!(pf.stream_id, expected_stream_id);
            assert_eq!(
                pf.stream_dependency,
                StreamId::from_wire(stream_dependency_raw)
            );
            let expected_weight = Weight::new(weight as u16).expect("valid weight");
            assert_eq!(pf.weight, expected_weight);
            assert_eq!(pf.exclusive, exclusive);
        } else {
            panic!("expected PRIORITY frame");
        }
        Ok(())
    })?;
    Ok(())
}

/// PRIORITY フレームが 5 バイトでないとエラー
#[test]
fn prop_priority_wrong_size_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let payload_len = match noprop::sample_usize_in(ctx, 0..2) {
            0 => noprop::sample_usize_in(ctx, 0..5),
            _ => 6 + noprop::sample_usize_in(ctx, 0..14),
        };
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x02); // PRIORITY frame type
        buf.push(0x00); // flags
        buf.push(((sid >> 24) & 0x7f) as u8);
        buf.push(((sid >> 16) & 0xff) as u8);
        buf.push(((sid >> 8) & 0xff) as u8);
        buf.push((sid & 0xff) as u8);
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        assert!(result.is_err(), "PRIORITY with wrong size should error");
        let err = result.unwrap_err();
        // RFC 9113 Section 6.3: PRIORITY フレームサイズ不正はストリームエラー
        assert!(
            err.is_stream_error(),
            "PRIORITY size error should be stream error, not connection error"
        );
        Ok(())
    })?;
    Ok(())
}

/// エンコードされたフレームの length フィールドはペイロード長と一致する
///
/// 数学的意義: length フィールドの整合性 (RFC 9113 Section 4.1)
#[test]
fn prop_encoded_length_field_matches_payload() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let data = sample_arbitrary_bytes(ctx, 500);
        let pad_length = if noprop::sample_bool(ctx) {
            Some(noprop::sample_u64_in(ctx, 0..=128) as u8)
        } else {
            None
        };
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data: data.clone(),
            pad_length,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        // length フィールドを抽出 (bytes 0-2, big-endian)
        let length_field =
            (u32::from(encoded[0]) << 16) | (u32::from(encoded[1]) << 8) | u32::from(encoded[2]);

        // 実際のペイロード長
        let actual_payload_len = encoded.len() - 9; // 9 = header size

        assert_eq!(
            length_field as usize, actual_payload_len,
            "length field must match actual payload length"
        );

        // パディング付きの場合の詳細検証
        if let Some(pad_len) = pad_length {
            // ペイロード = 1 (pad_length field) + data.len() + pad_len
            let expected = 1 + data.len() + pad_len as usize;
            assert_eq!(actual_payload_len, expected);
        } else {
            assert_eq!(actual_payload_len, data.len());
        }
        Ok(())
    })?;
    Ok(())
}

/// エンコードは冪等: 同じフレームを何度エンコードしても同じ結果
///
/// 数学的意義: エンコードの決定性
#[test]
fn prop_encode_idempotent() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let data = sample_arbitrary_bytes(ctx, 200);
        let end_stream = noprop::sample_bool(ctx);
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream,
            data,
            pad_length: None,
        });

        let mut encoder1 = FrameEncoder::new();
        encoder1.encode(&frame).expect("encode should succeed");
        let encoded1 = encoder1.take();

        let mut encoder2 = FrameEncoder::new();
        encoder2.encode(&frame).expect("encode should succeed");
        let encoded2 = encoder2.take();

        assert_eq!(
            encoded1, encoded2,
            "encoding same frame twice must produce identical bytes"
        );
        Ok(())
    })?;
    Ok(())
}

/// デコードは決定的: 同じバイト列をデコードすると常に同じ結果
///
/// 数学的意義: デコードの決定性
#[test]
fn prop_decode_deterministic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let data = sample_arbitrary_bytes(ctx, 200);
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data,
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        let mut decoder1 = FrameDecoder::new(16384);
        decoder1.feed(&encoded);
        let decoded1 = decoder1
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");

        let mut decoder2 = FrameDecoder::new(16384);
        decoder2.feed(&encoded);
        let decoded2 = decoder2
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");

        assert_eq!(
            decoded1, decoded2,
            "decoding same bytes twice must produce identical frames"
        );
        Ok(())
    })?;
    Ok(())
}

/// デコーダーの buffered_len は feed - 消費 と一致
///
/// 数学的意義: デコーダーのバッファ状態の不変条件
#[test]
fn prop_decoder_buffered_len_invariant() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let frame_count = noprop::sample_usize_in(ctx, 1..=4);
        let frames: Vec<(NonZeroStreamId, Vec<u8>)> = (0..frame_count)
            .map(|_| {
                (
                    sample_valid_stream_id(ctx),
                    sample_arbitrary_bytes(ctx, 100),
                )
            })
            .collect();
        let mut encoder = FrameEncoder::new();

        for &(stream_id, ref data) in &frames {
            let frame = Frame::Data(DataFrame {
                stream_id,
                end_stream: false,
                data: data.clone(),
                pad_length: None,
            });
            encoder.encode(&frame).expect("encode should succeed");
        }
        let encoded = encoder.take();
        let total_encoded_len = encoded.len();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        assert_eq!(decoder.buffered_len(), total_encoded_len);

        let mut consumed = 0;
        for _ in 0..frames.len() {
            let frame_start = consumed;
            let decoded = decoder.decode().expect("decode should succeed");
            assert!(decoded.is_some());

            // 消費されたバイト数を計算
            consumed = total_encoded_len - decoder.buffered_len();
            let frame_size = consumed - frame_start;

            // フレームサイズ = 9 (header) + payload
            assert!(frame_size >= 9);
        }

        // 全フレームデコード後、バッファは空
        assert_eq!(decoder.buffered_len(), 0);
        Ok(())
    })?;
    Ok(())
}

/// フレームの独立性: 連結されたフレームは個別にデコード可能
///
/// 数学的意義: フレーム境界の保存性
#[test]
fn prop_frame_independence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let frame_count = noprop::sample_usize_in(ctx, 2..=4);
        let frame_params: Vec<(NonZeroStreamId, u32)> = (0..frame_count)
            .map(|_| {
                let sid = sample_valid_stream_id(ctx);
                let inc = 1 + noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64 - 1) as u32;
                (sid, inc)
            })
            .collect();
        // 複数の WINDOW_UPDATE フレームを生成
        let frames: Vec<Frame> = frame_params
            .iter()
            .map(|&(sid, inc)| {
                let increment = WindowIncrement::new(inc).expect("valid window increment");
                Frame::WindowUpdate(WindowUpdateFrame::for_stream(sid, increment))
            })
            .collect();

        // 個別にエンコード
        let individual_encodings: Vec<Vec<u8>> = frames
            .iter()
            .map(|f| {
                let mut enc = FrameEncoder::new();
                enc.encode(f).expect("encode should succeed");
                enc.take()
            })
            .collect();

        // 連結してエンコード
        let mut combined_encoder = FrameEncoder::new();
        for frame in &frames {
            combined_encoder
                .encode(frame)
                .expect("encode should succeed");
        }
        let combined = combined_encoder.take();

        // 連結結果は個別エンコードの連結と一致
        let concatenated: Vec<u8> = individual_encodings.iter().flatten().copied().collect();
        assert_eq!(
            &combined, &concatenated,
            "combined encoding must equal concatenated individual encodings"
        );

        // 連結からデコードした結果は元のフレームと一致
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&combined);

        for (i, original) in frames.iter().enumerate() {
            let decoded = decoder
                .decode()
                .expect("decode should succeed")
                .expect("decode should succeed");
            assert_eq!(
                &decoded, original,
                "frame {} mismatch after decoding from combined",
                i
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// FLAGS の保存性: エンコード/デコードでフラグが保存される
///
/// 数学的意義: フラグビットの保存性
#[test]
fn prop_flags_preserved() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let end_stream = noprop::sample_bool(ctx);
        let end_headers = noprop::sample_bool(ctx);
        let header_block = sample_arbitrary_bytes(ctx, 100);
        let frame = Frame::Headers(HeadersFrame {
            stream_id,
            end_stream,
            end_headers,
            priority_fields: None,
            header_block_fragment: header_block,
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        // フラグフィールドを直接検証 (byte 4)
        let flags_byte = encoded[4];
        let end_stream_bit = (flags_byte & 0x01) != 0;
        let end_headers_bit = (flags_byte & 0x04) != 0;

        assert_eq!(end_stream_bit, end_stream, "END_STREAM flag mismatch");
        assert_eq!(end_headers_bit, end_headers, "END_HEADERS flag mismatch");

        // デコード後も一致
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");

        if let Frame::Headers(hf) = decoded {
            assert_eq!(hf.end_stream, end_stream);
            assert_eq!(hf.end_headers, end_headers);
        } else {
            panic!("expected HEADERS frame");
        }
        Ok(())
    })?;
    Ok(())
}

/// パディングの整合性: パディング長 + データ長 + 1 = ペイロード長
///
/// 数学的意義: パディング構造の不変条件
#[test]
fn prop_padding_structure_invariant() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_valid_stream_id(ctx);
        let data = sample_arbitrary_bytes(ctx, 200);
        let pad_length = noprop::sample_u64_in(ctx, 0..=100) as u8;
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data: data.clone(),
            pad_length: Some(pad_length),
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        // length フィールド
        let length_field =
            (u32::from(encoded[0]) << 16) | (u32::from(encoded[1]) << 8) | u32::from(encoded[2]);

        // PADDED フラグが設定されていることを確認
        let flags = encoded[4];
        assert!((flags & 0x08) != 0, "PADDED flag must be set");

        // ペイロード構造の検証
        // payload = [pad_length: 1 byte] [data: N bytes] [padding: pad_length bytes]
        let expected_payload_len = 1 + data.len() + pad_length as usize;
        assert_eq!(length_field as usize, expected_payload_len);

        // パディング長フィールドの値を検証
        let pad_length_field = encoded[9]; // ペイロードの最初のバイト
        assert_eq!(pad_length_field, pad_length);

        // デコード後のデータが正しいことを確認
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");

        if let Frame::Data(df) = decoded {
            assert_eq!(df.data, data, "data must be preserved after padding");
            assert_eq!(df.pad_length, Some(pad_length));
        } else {
            panic!("expected DATA frame");
        }
        Ok(())
    })?;
    Ok(())
}

/// stream_id の範囲制約: エンコード後も 31 ビット範囲内
///
/// 数学的意義: stream_id の範囲保存性
#[test]
fn prop_stream_id_31bit_range() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = StreamId::from_wire(noprop::sample_u64_in(ctx, 0..=0x7FFF_FFFFu64) as u32);
        let increment = WindowIncrement::new(1000).expect("valid window increment");
        let frame = match stream_id.non_zero() {
            Some(nz) => Frame::WindowUpdate(WindowUpdateFrame::for_stream(nz, increment)),
            None => Frame::WindowUpdate(WindowUpdateFrame::for_connection(increment)),
        };

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).expect("encode should succeed");
        let encoded = encoder.take();

        // stream_id フィールドを抽出 (bytes 5-8)
        // R ビット (最上位ビット) をマスクして取得
        let encoded_stream_id = ((u32::from(encoded[5]) & 0x7f) << 24)
            | (u32::from(encoded[6]) << 16)
            | (u32::from(encoded[7]) << 8)
            | u32::from(encoded[8]);

        assert_eq!(encoded_stream_id, stream_id.as_u32());

        // 最上位ビット (R ビット) は 0
        assert_eq!(encoded[5] & 0x80, 0, "R bit must be 0");
        Ok(())
    })?;
    Ok(())
}

mod from_static_consistency {
    use shiguredo_http2::{
        ClientStreamId, LastStreamId, NonZeroStreamId, ServerStreamId, Weight, WindowIncrement,
    };

    const SEED_ENV: &str = "HTTP2_PBT_SEED";
    const CASES: usize = 256;

    /// WindowIncrement::from_static と new の一貫性
    #[test]
    fn prop_window_increment_static_matches_new() -> noprop::TestResult {
        let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
        let mut runner = noprop::Runner::new(seed);
        runner.run(CASES, |ctx| {
            let v = noprop::sample_u64_in(ctx, 1..=WindowIncrement::MAX as u64) as u32;
            let via_new = WindowIncrement::new(v).expect("construction should succeed");
            let via_static = WindowIncrement::from_static(v);
            assert_eq!(via_new, via_static);
            Ok(())
        })?;
        Ok(())
    }

    /// Weight::from_static と new の一貫性
    #[test]
    fn prop_weight_static_matches_new() -> noprop::TestResult {
        let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
        let mut runner = noprop::Runner::new(seed);
        runner.run(CASES, |ctx| {
            let w = noprop::sample_u64_in(ctx, 0..=255) as u16;
            let via_new = Weight::new(w).expect("construction should succeed");
            let via_static = Weight::from_static(w);
            assert_eq!(via_new, via_static);
            Ok(())
        })?;
        Ok(())
    }

    /// LastStreamId::from_static と new の一貫性
    #[test]
    fn prop_last_stream_id_static_matches_new() -> noprop::TestResult {
        let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
        let mut runner = noprop::Runner::new(seed);
        runner.run(CASES, |ctx| {
            let id = noprop::sample_u64_in(ctx, 0..=LastStreamId::MAX as u64) as u32;
            let via_new = LastStreamId::new(id).expect("construction should succeed");
            let via_static = LastStreamId::from_static(id);
            assert_eq!(via_new, via_static);
            Ok(())
        })?;
        Ok(())
    }

    /// ClientStreamId::from_static と new の一貫性
    #[test]
    fn prop_client_stream_id_static_matches_new() -> noprop::TestResult {
        let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
        let mut runner = noprop::Runner::new(seed);
        runner.run(CASES, |ctx| {
            // 奇数 ID を valid-by-construction で生成する
            let odd = 2 * noprop::sample_u64_in(ctx, 0..=0x3FFF_FFFFu64) as u32 + 1;
            let via_new = ClientStreamId::new(odd).expect("construction should succeed");
            let via_static = ClientStreamId::from_static(odd);
            assert_eq!(via_new, via_static);
            Ok(())
        })?;
        Ok(())
    }

    /// ServerStreamId::from_static と new の一貫性
    #[test]
    fn prop_server_stream_id_static_matches_new() -> noprop::TestResult {
        let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
        let mut runner = noprop::Runner::new(seed);
        runner.run(CASES, |ctx| {
            // 偶数 ID を valid-by-construction で生成する (2..=2^31-2)
            let even = 2 * noprop::sample_u64_in(ctx, 0..=0x3FFF_FFFEu64) as u32 + 2;
            let via_new = ServerStreamId::new(even).expect("construction should succeed");
            let via_static = ServerStreamId::from_static(even);
            assert_eq!(via_new, via_static);
            Ok(())
        })?;
        Ok(())
    }

    /// NonZeroStreamId::from_static と new の一貫性
    #[test]
    fn prop_non_zero_stream_id_static_matches_new() -> noprop::TestResult {
        let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
        let mut runner = noprop::Runner::new(seed);
        runner.run(CASES, |ctx| {
            let id = 1 + noprop::sample_u64_in(ctx, 0..0x7FFF_FFFFu64) as u32;
            let via_new = NonZeroStreamId::new(id).expect("construction should succeed");
            let via_static = NonZeroStreamId::from_static(id);
            assert_eq!(via_new, via_static);
            Ok(())
        })?;
        Ok(())
    }
}
