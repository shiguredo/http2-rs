//! フレームエンコード/デコードの PBT
//!
//! RFC 9113 Section 4, 6 に基づくフレームのエンコード/デコードを検証する。

use proptest::prelude::*;
use shiguredo_http2::frame::{ContinuationFrame, FrameFlags, FrameHeader, PriorityUpdateFrame};
use shiguredo_http2::settings::{MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, MIN_MAX_FRAME_SIZE};
use shiguredo_http2::{
    DataFrame, Frame, FrameDecoder, FrameEncoder, GoawayFrame, HeadersFrame, MaxFrameSize,
    PingFrame, RstStreamFrame, Setting, SettingsFrame, StreamId, WindowSize, WindowUpdateFrame,
};

/// 有効なストリーム ID を生成する（0 以外）
fn valid_stream_id() -> impl Strategy<Value = StreamId> {
    (1..=0x7FFF_FFFFu32).prop_map(StreamId::from_wire)
}

/// 有効な Setting を生成する
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

/// 任意のバイト列を生成する
fn arbitrary_bytes(max_len: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..=max_len)
}

proptest! {
    /// DATA フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_data_frame_roundtrip(
        stream_id in valid_stream_id(),
        end_stream in any::<bool>(),
        data in arbitrary_bytes(1024),
    ) {
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream,
            data: data.clone(),
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Data(df) = decoded {
            prop_assert_eq!(df.stream_id, stream_id);
            prop_assert_eq!(df.end_stream, end_stream);
            prop_assert_eq!(df.data, data);
        } else {
            panic!("expected DATA frame");
        }
    }

    /// HEADERS フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_headers_frame_roundtrip(
        stream_id in valid_stream_id(),
        end_stream in any::<bool>(),
        end_headers in any::<bool>(),
        header_block in arbitrary_bytes(512),
    ) {
        let frame = Frame::Headers(HeadersFrame {
            stream_id,
            end_stream,
            end_headers,
            priority_fields: None,
            header_block_fragment: header_block.clone(),
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Headers(hf) = decoded {
            prop_assert_eq!(hf.stream_id, stream_id);
            prop_assert_eq!(hf.end_stream, end_stream);
            prop_assert_eq!(hf.end_headers, end_headers);
            prop_assert_eq!(hf.header_block_fragment, header_block);
        } else {
            panic!("expected HEADERS frame");
        }
    }

    /// RST_STREAM フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_rst_stream_frame_roundtrip(
        stream_id in valid_stream_id(),
        error_code in any::<u32>(),
    ) {
        let frame = Frame::RstStream(RstStreamFrame {
            stream_id,
            error_code,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::RstStream(rf) = decoded {
            prop_assert_eq!(rf.stream_id, stream_id);
            prop_assert_eq!(rf.error_code, error_code);
        } else {
            panic!("expected RST_STREAM frame");
        }
    }

    /// SETTINGS フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_settings_frame_roundtrip(ack in any::<bool>()) {
        let frame = if ack {
            Frame::Settings(SettingsFrame::ack())
        } else {
            let mut sf = SettingsFrame::new();
            sf.add(Setting::HeaderTableSize(4096));
            sf.add(Setting::InitialWindowSize(shiguredo_http2::WindowSize::from_static(65535)));
            Frame::Settings(sf)
        };

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Settings(sf) = decoded {
            prop_assert_eq!(sf.is_ack(), ack);
        } else {
            panic!("expected SETTINGS frame");
        }
    }

    /// PING フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_ping_frame_roundtrip(
        ack in any::<bool>(),
        opaque_data in any::<[u8; 8]>(),
    ) {
        let frame = Frame::Ping(PingFrame { ack, opaque_data });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Ping(pf) = decoded {
            prop_assert_eq!(pf.ack, ack);
            prop_assert_eq!(pf.opaque_data, opaque_data);
        } else {
            panic!("expected PING frame");
        }
    }

    /// GOAWAY フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_goaway_frame_roundtrip(
        last_stream_id in (0..=0x7FFF_FFFFu32).prop_map(StreamId::from_wire),
        error_code in any::<u32>(),
        debug_data in arbitrary_bytes(128),
    ) {
        let frame = Frame::Goaway(GoawayFrame {
            last_stream_id,
            error_code,
            debug_data: debug_data.clone(),
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Goaway(gf) = decoded {
            prop_assert_eq!(gf.last_stream_id, last_stream_id);
            prop_assert_eq!(gf.error_code, error_code);
            prop_assert_eq!(gf.debug_data, debug_data);
        } else {
            panic!("expected GOAWAY frame");
        }
    }

    /// WINDOW_UPDATE フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_window_update_frame_roundtrip(
        stream_id in (0..=0x7FFF_FFFFu32).prop_map(StreamId::from_wire),
        // WINDOW_UPDATE の increment は 1 以上でなければならない
        window_size_increment in 1..=0x7FFF_FFFFu32,
    ) {
        let frame = Frame::WindowUpdate(WindowUpdateFrame {
            stream_id,
            window_size_increment,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::WindowUpdate(wuf) = decoded {
            prop_assert_eq!(wuf.stream_id, stream_id);
            prop_assert_eq!(wuf.window_size_increment, window_size_increment);
        } else {
            panic!("expected WINDOW_UPDATE frame");
        }
    }

    /// デコーダーの堅牢性テスト（任意のバイト列に対してパニックしない）
    #[test]
    fn prop_decoder_robustness(data in arbitrary_bytes(256)) {
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&data);

        // デコードを試みる（結果は気にしない、パニックしないことを確認）
        let _ = decoder.decode();
    }

    // ========================================
    // CONTINUATION フレームのテスト
    // ========================================

    /// CONTINUATION フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_continuation_frame_roundtrip(
        stream_id in valid_stream_id(),
        end_headers in any::<bool>(),
        header_block in arbitrary_bytes(512),
    ) {
        let frame = Frame::Continuation(ContinuationFrame {
            stream_id,
            end_headers,
            header_block_fragment: header_block.clone(),
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Continuation(cf) = decoded {
            prop_assert_eq!(cf.stream_id, stream_id);
            prop_assert_eq!(cf.end_headers, end_headers);
            prop_assert_eq!(cf.header_block_fragment, header_block);
        } else {
            panic!("expected CONTINUATION frame");
        }
    }

    // ========================================
    // PRIORITY_UPDATE フレームのテスト (RFC 9218)
    // ========================================

    /// PRIORITY_UPDATE フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_priority_update_frame_roundtrip(
        prioritized_element_id in (0..=0x7FFF_FFFFu32).prop_map(StreamId::from_wire),
        priority_field_value in arbitrary_bytes(128),
    ) {
        let frame = Frame::PriorityUpdate(PriorityUpdateFrame {
            prioritized_element_id,
            priority_field_value: priority_field_value.clone(),
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::PriorityUpdate(puf) = decoded {
            prop_assert_eq!(puf.prioritized_element_id, prioritized_element_id);
            prop_assert_eq!(puf.priority_field_value, priority_field_value);
        } else {
            panic!("expected PRIORITY_UPDATE frame");
        }
    }

    // ========================================
    // Unknown フレームのテスト
    // ========================================

    /// Unknown フレームのエンコード/デコード往復テスト
    ///
    /// RFC 9113 Section 4.1: 未知のフレームタイプは無視すべき
    #[test]
    fn prop_unknown_frame_roundtrip(
        // 未知のフレームタイプ (0x0a-0x0f, 0x11-0xff)
        // 注: 0x05 (PUSH_PROMISE) は既知のフレームタイプとして処理される
        frame_type in prop_oneof![
            (0x0au8..=0x0f),  // 0x0a-0x0f は未定義
            (0x11u8..=0xff),  // 0x11-0xff は未定義
        ],
        stream_id in 0..=0x7FFF_FFFFu32,
        payload in arbitrary_bytes(128),
    ) {
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

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Unknown { header: h, payload: p } = decoded {
            prop_assert_eq!(h.frame_type, frame_type);
            prop_assert_eq!(h.stream_id, stream_id);
            prop_assert_eq!(p, payload);
        } else {
            panic!("expected Unknown frame");
        }
    }

    // ========================================
    // パディング付きフレームのテスト
    // ========================================

    /// パディング付き DATA フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_data_frame_with_padding_roundtrip(
        stream_id in valid_stream_id(),
        end_stream in any::<bool>(),
        data in arbitrary_bytes(512),
        pad_length in 0..=128u8,
    ) {
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream,
            data: data.clone(),
            pad_length: Some(pad_length),
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Data(df) = decoded {
            prop_assert_eq!(df.stream_id, stream_id);
            prop_assert_eq!(df.end_stream, end_stream);
            prop_assert_eq!(df.data, data);
            prop_assert_eq!(df.pad_length, Some(pad_length));
        } else {
            panic!("expected DATA frame");
        }
    }

    /// パディング付き HEADERS フレームのエンコード/デコード往復テスト
    #[test]
    fn prop_headers_frame_with_padding_roundtrip(
        stream_id in valid_stream_id(),
        end_stream in any::<bool>(),
        end_headers in any::<bool>(),
        header_block in arbitrary_bytes(256),
        pad_length in 0..=64u8,
    ) {
        let frame = Frame::Headers(HeadersFrame {
            stream_id,
            end_stream,
            end_headers,
            priority_fields: None,
            header_block_fragment: header_block.clone(),
            pad_length: Some(pad_length),
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Headers(hf) = decoded {
            prop_assert_eq!(hf.stream_id, stream_id);
            prop_assert_eq!(hf.end_stream, end_stream);
            prop_assert_eq!(hf.end_headers, end_headers);
            prop_assert_eq!(hf.header_block_fragment, header_block);
            prop_assert_eq!(hf.pad_length, Some(pad_length));
        } else {
            panic!("expected HEADERS frame");
        }
    }

    // ========================================
    // 複数フレームの連続デコードテスト
    // ========================================

    /// 複数フレームの連続デコード
    ///
    /// 数学的意義: デコーダーの状態遷移の正当性
    #[test]
    fn prop_multiple_frames_decode(
        frame_count in 2..10usize,
        stream_ids in prop::collection::vec(valid_stream_id(), 2..10),
    ) {
        let mut encoder = FrameEncoder::new();

        // 複数フレームをエンコード
        let frames: Vec<Frame> = stream_ids.iter().take(frame_count).map(|sid| {
            Frame::WindowUpdate(WindowUpdateFrame {
                stream_id: *sid,
                window_size_increment: 1000,
            })
        }).collect();

        for frame in &frames {
            encoder.encode(frame).unwrap();
        }
        let encoded = encoder.take();

        // 連続デコード
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);

        for (i, original) in frames.iter().enumerate() {
            let decoded = decoder.decode().unwrap();
            prop_assert!(
                decoded.is_some(),
                "Frame {} should be decoded",
                i
            );
            prop_assert_eq!(
                decoded.unwrap().stream_id(),
                original.stream_id(),
                "Frame {} stream_id mismatch",
                i
            );
        }

        // これ以上フレームがないことを確認
        prop_assert!(decoder.decode().unwrap().is_none());
    }

    // ========================================
    // 部分的データの feed テスト
    // ========================================

    /// 部分的データの feed でのデコード
    ///
    /// 数学的意義: ストリーミングデコードの正当性
    #[test]
    fn prop_partial_feed_decode(
        stream_id in valid_stream_id(),
        data in arbitrary_bytes(100),
    ) {
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data: data.clone(),
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);

        // 1 バイトずつ feed
        for (i, byte) in encoded.iter().enumerate() {
            decoder.feed(&[*byte]);

            // 最後のバイトまではデコードできない
            if i < encoded.len() - 1 {
                let result = decoder.decode().unwrap();
                prop_assert!(
                    result.is_none(),
                    "Should not decode until all bytes are fed (at byte {})",
                    i
                );
            }
        }

        // 全バイト feed 後はデコードできる
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(decoded.stream_id(), stream_id);
    }

    // ========================================
    // SETTINGS フレームの詳細テスト
    // ========================================

    /// SETTINGS フレームの設定値エンコード/デコード
    #[test]
    fn prop_settings_frame_values_roundtrip(
        settings in prop::collection::vec(valid_setting(), 1..10),
    ) {
        let frame = Frame::Settings(SettingsFrame::from_settings(settings.clone()));

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Settings(sf) = decoded {
            prop_assert!(!sf.is_ack());
            prop_assert_eq!(sf.settings().len(), settings.len());
            for (orig, decoded) in settings.iter().zip(sf.settings().iter()) {
                prop_assert_eq!(orig.as_wire(), decoded.as_wire());
            }
        } else {
            panic!("expected SETTINGS frame");
        }
    }

    // ========================================
    // フレームサイズ超過の検出テスト
    // ========================================

    /// フレームサイズ超過の検出
    ///
    /// 数学的意義: max_frame_size 制約の検証
    #[test]
    fn prop_frame_size_exceeded_detected(
        max_frame_size in 100..1000u32,
        data_len in 1000..2000usize,
    ) {
        prop_assume!(data_len > max_frame_size as usize);

        let frame = Frame::Data(DataFrame {
            stream_id: StreamId::from_wire(1),
            end_stream: false,
            data: vec![0u8; data_len],
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder = FrameDecoder::new(max_frame_size);
        decoder.feed(&encoded);

        // デコード時にエラーが発生する
        let result = decoder.decode();
        prop_assert!(result.is_err(), "Should reject frame exceeding max_frame_size");
    }

    // ========================================
    // フレームヘッダーの不変条件テスト
    // ========================================

    /// エンコードされたフレームヘッダーは 9 バイト
    ///
    /// 数学的意義: フレームヘッダーサイズの不変条件
    #[test]
    fn prop_frame_header_size_invariant(
        stream_id in valid_stream_id(),
        data in arbitrary_bytes(100),
    ) {
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data: data.clone(),
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        // フレームヘッダー (9 バイト) + ペイロード
        prop_assert_eq!(encoded.len(), 9 + data.len());
    }

    /// ストリーム ID の上位ビットは予約済み
    ///
    /// RFC 9113 Section 4.1: R ビットは予約済み (0)
    #[test]
    fn prop_stream_id_reserved_bit(
        stream_id in (0..=0x7FFF_FFFFu32).prop_map(StreamId::from_wire),
    ) {
        let frame = Frame::WindowUpdate(WindowUpdateFrame {
            stream_id,
            window_size_increment: 1000,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        // フレームヘッダーの 5 バイト目 (オフセット 5) の上位ビットは 0
        prop_assert_eq!(encoded[5] & 0x80, 0, "Reserved bit must be 0");

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert_eq!(decoded.stream_id(), stream_id);
    }

    // ========================================
    // デコーダーエラーケースのテスト
    // ========================================

    /// DATA フレームの stream ID 0 はエラー
    #[test]
    fn prop_data_stream_id_zero_error(
        data in arbitrary_bytes(100),
    ) {
        // 手動でフレームを構築 (stream_id = 0)
        let mut buf = Vec::new();
        let length = data.len() as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x00);  // DATA frame type
        buf.push(0x00);  // flags
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend_from_slice(&data);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "DATA frame with stream_id 0 should error");
    }

    /// HEADERS フレームの stream ID 0 はエラー
    #[test]
    fn prop_headers_stream_id_zero_error(
        header_block in arbitrary_bytes(50),
    ) {
        let mut buf = Vec::new();
        let length = header_block.len() as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x01);  // HEADERS frame type
        buf.push(0x04);  // END_HEADERS flag
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend_from_slice(&header_block);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "HEADERS frame with stream_id 0 should error");
    }

    /// RST_STREAM フレームの stream ID 0 はエラー
    #[test]
    fn prop_rst_stream_stream_id_zero_error(
        error_code in any::<u32>(),
    ) {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 4]);  // length = 4
        buf.push(0x03);  // RST_STREAM frame type
        buf.push(0x00);  // flags
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend_from_slice(&error_code.to_be_bytes());

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "RST_STREAM frame with stream_id 0 should error");
    }

    /// CONTINUATION フレームの stream ID 0 はエラー
    #[test]
    fn prop_continuation_stream_id_zero_error(
        header_block in arbitrary_bytes(50),
    ) {
        let mut buf = Vec::new();
        let length = header_block.len() as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x09);  // CONTINUATION frame type
        buf.push(0x04);  // END_HEADERS flag
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend_from_slice(&header_block);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "CONTINUATION frame with stream_id 0 should error");
    }

    /// SETTINGS フレームの stream ID != 0 はエラー
    #[test]
    fn prop_settings_non_zero_stream_id_error(
        stream_id in valid_stream_id(),
    ) {
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 0]);  // length = 0 (ACK)
        buf.push(0x04);  // SETTINGS frame type
        buf.push(0x01);  // ACK flag
        // stream_id (非ゼロ)
        buf.push(((sid >> 24) & 0x7f) as u8);
        buf.push(((sid >> 16) & 0xff) as u8);
        buf.push(((sid >> 8) & 0xff) as u8);
        buf.push((sid & 0xff) as u8);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "SETTINGS frame with non-zero stream_id should error");
    }

    /// PING フレームの stream ID != 0 はエラー
    #[test]
    fn prop_ping_non_zero_stream_id_error(
        stream_id in valid_stream_id(),
        opaque_data in any::<[u8; 8]>(),
    ) {
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 8]);  // length = 8
        buf.push(0x06);  // PING frame type
        buf.push(0x00);  // flags
        // stream_id (非ゼロ)
        buf.push(((sid >> 24) & 0x7f) as u8);
        buf.push(((sid >> 16) & 0xff) as u8);
        buf.push(((sid >> 8) & 0xff) as u8);
        buf.push((sid & 0xff) as u8);
        buf.extend_from_slice(&opaque_data);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "PING frame with non-zero stream_id should error");
    }

    /// GOAWAY フレームの stream ID != 0 はエラー
    #[test]
    fn prop_goaway_non_zero_stream_id_error(
        stream_id in valid_stream_id(),
        last_stream_id in 0..=0x7FFF_FFFFu32,
        error_code in any::<u32>(),
    ) {
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 8]);  // length = 8
        buf.push(0x07);  // GOAWAY frame type
        buf.push(0x00);  // flags
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

        prop_assert!(result.is_err(), "GOAWAY frame with non-zero stream_id should error");
    }

    /// PRIORITY_UPDATE フレームの stream ID != 0 はエラー
    #[test]
    fn prop_priority_update_non_zero_stream_id_error(
        stream_id in valid_stream_id(),
        prioritized_element_id in 0..=0x7FFF_FFFFu32,
    ) {
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 4]);  // length = 4
        buf.push(0x10);  // PRIORITY_UPDATE frame type
        buf.push(0x00);  // flags
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

        prop_assert!(result.is_err(), "PRIORITY_UPDATE frame with non-zero stream_id should error");
    }

    /// WINDOW_UPDATE の increment が 0 はエラー
    ///
    /// RFC 9113 Section 6.9: increment of 0 is a protocol error
    #[test]
    fn prop_window_update_zero_increment_error(
        stream_id in 0..=0x7FFF_FFFFu32,
    ) {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 4]);  // length = 4
        buf.push(0x08);  // WINDOW_UPDATE frame type
        buf.push(0x00);  // flags
        // stream_id
        buf.push(((stream_id >> 24) & 0x7f) as u8);
        buf.push(((stream_id >> 16) & 0xff) as u8);
        buf.push(((stream_id >> 8) & 0xff) as u8);
        buf.push((stream_id & 0xff) as u8);
        // increment = 0
        buf.extend_from_slice(&[0, 0, 0, 0]);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "WINDOW_UPDATE with zero increment should error");
    }

    /// SETTINGS ACK で空でないペイロードはエラー
    ///
    /// RFC 9113 Section 6.5: ACK with non-empty payload is an error
    #[test]
    fn prop_settings_ack_non_empty_payload_error(
        payload_len in 1..100usize,
    ) {
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x04);  // SETTINGS frame type
        buf.push(0x01);  // ACK flag
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "SETTINGS ACK with non-empty payload should error");
    }

    /// SETTINGS ペイロードが 6 の倍数でないとエラー
    ///
    /// RFC 9113 Section 6.5: payload must be multiple of 6
    #[test]
    fn prop_settings_payload_not_multiple_of_6_error(
        // 6 の倍数でない長さ
        extra_bytes in 1..5usize,
        setting_count in 0..5usize,
    ) {
        let payload_len = setting_count * 6 + extra_bytes;
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x04);  // SETTINGS frame type
        buf.push(0x00);  // no ACK flag
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "SETTINGS payload not multiple of 6 should error");
    }

    /// RST_STREAM ペイロードが 4 バイトでないとエラー
    ///
    /// RFC 9113 Section 6.4: RST_STREAM must be exactly 4 bytes
    #[test]
    fn prop_rst_stream_wrong_size_error(
        payload_len in prop_oneof![0..4usize, 5..20usize],
    ) {
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x03);  // RST_STREAM frame type
        buf.push(0x00);  // flags
        buf.extend_from_slice(&[0, 0, 0, 1]);  // stream_id = 1
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "RST_STREAM with wrong size should error");
    }

    /// PING ペイロードが 8 バイトでないとエラー
    ///
    /// RFC 9113 Section 6.7: PING must be exactly 8 bytes
    #[test]
    fn prop_ping_wrong_size_error(
        payload_len in prop_oneof![0..8usize, 9..20usize],
    ) {
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x06);  // PING frame type
        buf.push(0x00);  // flags
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "PING with wrong size should error");
    }

    /// GOAWAY ペイロードが 8 バイト未満はエラー
    ///
    /// RFC 9113 Section 6.8: GOAWAY must be at least 8 bytes
    #[test]
    fn prop_goaway_too_short_error(
        payload_len in 0..8usize,
    ) {
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x07);  // GOAWAY frame type
        buf.push(0x00);  // flags
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "GOAWAY with less than 8 bytes should error");
    }

    /// WINDOW_UPDATE ペイロードが 4 バイトでないとエラー
    ///
    /// RFC 9113 Section 6.9: WINDOW_UPDATE must be exactly 4 bytes
    #[test]
    fn prop_window_update_wrong_size_error(
        payload_len in prop_oneof![0..4usize, 5..20usize],
    ) {
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x08);  // WINDOW_UPDATE frame type
        buf.push(0x00);  // flags
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "WINDOW_UPDATE with wrong size should error");
    }

    /// PRIORITY_UPDATE ペイロードが 4 バイト未満はエラー
    ///
    /// RFC 9218 Section 7.1: PRIORITY_UPDATE must be at least 4 bytes
    #[test]
    fn prop_priority_update_too_short_error(
        payload_len in 0..4usize,
    ) {
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x10);  // PRIORITY_UPDATE frame type
        buf.push(0x00);  // flags
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "PRIORITY_UPDATE with less than 4 bytes should error");
    }

    // ========================================
    // PRIORITY フレームのテスト (非推奨)
    // ========================================

    /// PRIORITY フレームのデコードテスト (非推奨、受信は処理する)
    #[test]
    fn prop_priority_frame_decode(
        stream_id_raw in 1..=0x7FFF_FFFFu32,
        stream_dependency_raw in 0..=0x7FFF_FFFFu32,
        weight in any::<u8>(),
        exclusive in any::<bool>(),
    ) {
        // 手動で PRIORITY フレームを構築
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 5]);  // length = 5
        buf.push(0x02);  // PRIORITY frame type
        buf.push(0x00);  // flags
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
        let result = decoder.decode().unwrap().unwrap();

        if let Frame::Priority(pf) = result {
            prop_assert_eq!(pf.stream_id, StreamId::from_wire(stream_id_raw));
            prop_assert_eq!(pf.stream_dependency, StreamId::from_wire(stream_dependency_raw));
            prop_assert_eq!(pf.weight, weight);
            prop_assert_eq!(pf.exclusive, exclusive);
        } else {
            panic!("expected PRIORITY frame");
        }
    }

    /// PRIORITY フレームの stream ID 0 はエラー
    #[test]
    fn prop_priority_stream_id_zero_error(
        stream_dependency in 0..=0x7FFF_FFFFu32,
        weight in any::<u8>(),
    ) {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0, 0, 5]);  // length = 5
        buf.push(0x02);  // PRIORITY frame type
        buf.push(0x00);  // flags
        buf.extend_from_slice(&[0, 0, 0, 0]);  // stream_id = 0
        buf.push(((stream_dependency >> 24) & 0x7f) as u8);
        buf.push(((stream_dependency >> 16) & 0xff) as u8);
        buf.push(((stream_dependency >> 8) & 0xff) as u8);
        buf.push((stream_dependency & 0xff) as u8);
        buf.push(weight);

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "PRIORITY frame with stream_id 0 should error");
    }

    /// PRIORITY フレームが 5 バイトでないとエラー
    #[test]
    fn prop_priority_wrong_size_error(
        stream_id in valid_stream_id(),
        payload_len in prop_oneof![0..5usize, 6..20usize],
    ) {
        let sid = stream_id.as_u32();
        let mut buf = Vec::new();
        let length = payload_len as u32;
        buf.push(((length >> 16) & 0xff) as u8);
        buf.push(((length >> 8) & 0xff) as u8);
        buf.push((length & 0xff) as u8);
        buf.push(0x02);  // PRIORITY frame type
        buf.push(0x00);  // flags
        buf.push(((sid >> 24) & 0x7f) as u8);
        buf.push(((sid >> 16) & 0xff) as u8);
        buf.push(((sid >> 8) & 0xff) as u8);
        buf.push((sid & 0xff) as u8);
        buf.extend(std::iter::repeat_n(0u8, payload_len));

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();

        prop_assert!(result.is_err(), "PRIORITY with wrong size should error");
        let err = result.unwrap_err();
        // RFC 9113 Section 6.3: PRIORITY フレームサイズ不正はストリームエラー
        prop_assert!(err.is_stream_error(), "PRIORITY size error should be stream error, not connection error");
    }

    // ========================================
    // 真の PBT: 数学的性質の検証
    // ========================================

    /// エンコードされたフレームの length フィールドはペイロード長と一致する
    ///
    /// 数学的意義: length フィールドの整合性 (RFC 9113 Section 4.1)
    #[test]
    fn prop_encoded_length_field_matches_payload(
        stream_id in valid_stream_id(),
        data in arbitrary_bytes(500),
        pad_length in prop::option::of(0..128u8),
    ) {
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data: data.clone(),
            pad_length,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        // length フィールドを抽出 (bytes 0-2, big-endian)
        let length_field = (u32::from(encoded[0]) << 16)
            | (u32::from(encoded[1]) << 8)
            | u32::from(encoded[2]);

        // 実際のペイロード長
        let actual_payload_len = encoded.len() - 9;  // 9 = header size

        prop_assert_eq!(
            length_field as usize,
            actual_payload_len,
            "length field must match actual payload length"
        );

        // パディング付きの場合の詳細検証
        if let Some(pad_len) = pad_length {
            // ペイロード = 1 (pad_length field) + data.len() + pad_len
            let expected = 1 + data.len() + pad_len as usize;
            prop_assert_eq!(actual_payload_len, expected);
        } else {
            prop_assert_eq!(actual_payload_len, data.len());
        }
    }

    /// エンコードされたフレームの frame_type フィールドは正しい
    ///
    /// 数学的意義: frame_type の保存性
    #[test]
    fn prop_encoded_frame_type_correct(
        stream_id in valid_stream_id(),
        data in arbitrary_bytes(100),
    ) {
        let test_cases: Vec<(Frame, u8)> = vec![
            (Frame::Data(DataFrame { stream_id, end_stream: false, data: data.clone(), pad_length: None }), 0x00),
            (Frame::Headers(HeadersFrame { stream_id, end_stream: false, end_headers: true, priority_fields: None, header_block_fragment: data.clone(), pad_length: None }), 0x01),
            (Frame::RstStream(RstStreamFrame { stream_id, error_code: 0 }), 0x03),
            (Frame::Settings(SettingsFrame::new()), 0x04),
            (Frame::Ping(PingFrame { ack: false, opaque_data: [0; 8] }), 0x06),
            (Frame::Goaway(GoawayFrame { last_stream_id: StreamId::Connection, error_code: 0, debug_data: vec![] }), 0x07),
            (Frame::WindowUpdate(WindowUpdateFrame { stream_id, window_size_increment: 1 }), 0x08),
            (Frame::Continuation(ContinuationFrame { stream_id, end_headers: true, header_block_fragment: data.clone() }), 0x09),
            (Frame::PriorityUpdate(PriorityUpdateFrame { prioritized_element_id: stream_id, priority_field_value: vec![] }), 0x10),
        ];

        for (frame, expected_type) in test_cases {
            let mut encoder = FrameEncoder::new();
            encoder.encode(&frame).unwrap();
            let encoded = encoder.take();

            // frame_type フィールド (byte 3)
            let frame_type_field = encoded[3];
            prop_assert_eq!(
                frame_type_field,
                expected_type,
                "frame_type field mismatch for {:?}",
                frame.frame_type()
            );
        }
    }

    /// エンコードは冪等: 同じフレームを何度エンコードしても同じ結果
    ///
    /// 数学的意義: エンコードの決定性
    #[test]
    fn prop_encode_idempotent(
        stream_id in valid_stream_id(),
        data in arbitrary_bytes(200),
        end_stream in any::<bool>(),
    ) {
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream,
            data,
            pad_length: None,
        });

        let mut encoder1 = FrameEncoder::new();
        encoder1.encode(&frame).unwrap();
        let encoded1 = encoder1.take();

        let mut encoder2 = FrameEncoder::new();
        encoder2.encode(&frame).unwrap();
        let encoded2 = encoder2.take();

        prop_assert_eq!(encoded1, encoded2, "encoding same frame twice must produce identical bytes");
    }

    /// デコードは決定的: 同じバイト列をデコードすると常に同じ結果
    ///
    /// 数学的意義: デコードの決定性
    #[test]
    fn prop_decode_deterministic(
        stream_id in valid_stream_id(),
        data in arbitrary_bytes(200),
    ) {
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data,
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        let mut decoder1 = FrameDecoder::new(16384);
        decoder1.feed(&encoded);
        let decoded1 = decoder1.decode().unwrap().unwrap();

        let mut decoder2 = FrameDecoder::new(16384);
        decoder2.feed(&encoded);
        let decoded2 = decoder2.decode().unwrap().unwrap();

        prop_assert_eq!(decoded1, decoded2, "decoding same bytes twice must produce identical frames");
    }

    /// デコーダーの buffered_len は feed - 消費 と一致
    ///
    /// 数学的意義: デコーダーのバッファ状態の不変条件
    #[test]
    fn prop_decoder_buffered_len_invariant(
        frames in prop::collection::vec(
            (valid_stream_id(), arbitrary_bytes(100)),
            1..5
        ),
    ) {
        let mut encoder = FrameEncoder::new();

        for &(stream_id, ref data) in &frames {
            let frame = Frame::Data(DataFrame {
                stream_id,
                end_stream: false,
                data: data.clone(),
                pad_length: None,
            });
            encoder.encode(&frame).unwrap();
        }
        let encoded = encoder.take();
        let total_encoded_len = encoded.len();

        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        prop_assert_eq!(decoder.buffered_len(), total_encoded_len);

        let mut consumed = 0;
        for _ in 0..frames.len() {
            let frame_start = consumed;
            let decoded = decoder.decode().unwrap();
            prop_assert!(decoded.is_some());

            // 消費されたバイト数を計算
            consumed = total_encoded_len - decoder.buffered_len();
            let frame_size = consumed - frame_start;

            // フレームサイズ = 9 (header) + payload
            prop_assert!(frame_size >= 9);
        }

        // 全フレームデコード後、バッファは空
        prop_assert_eq!(decoder.buffered_len(), 0);
    }

    /// フレームの独立性: 連結されたフレームは個別にデコード可能
    ///
    /// 数学的意義: フレーム境界の保存性
    #[test]
    fn prop_frame_independence(
        frame_params in prop::collection::vec(
            (valid_stream_id(), 1..=0x7FFF_FFFFu32),
            2..5
        ),
    ) {
        // 複数の WINDOW_UPDATE フレームを生成
        let frames: Vec<Frame> = frame_params.iter().map(|&(sid, inc)| {
            Frame::WindowUpdate(WindowUpdateFrame {
                stream_id: sid,
                window_size_increment: inc,
            })
        }).collect();

        // 個別にエンコード
        let individual_encodings: Vec<Vec<u8>> = frames.iter().map(|f| {
            let mut enc = FrameEncoder::new();
            enc.encode(f).unwrap();
            enc.take()
        }).collect();

        // 連結してエンコード
        let mut combined_encoder = FrameEncoder::new();
        for frame in &frames {
            combined_encoder.encode(frame).unwrap();
        }
        let combined = combined_encoder.take();

        // 連結結果は個別エンコードの連結と一致
        let concatenated: Vec<u8> = individual_encodings.iter().flatten().copied().collect();
        prop_assert_eq!(&combined, &concatenated, "combined encoding must equal concatenated individual encodings");

        // 連結からデコードした結果は元のフレームと一致
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&combined);

        for (i, original) in frames.iter().enumerate() {
            let decoded = decoder.decode().unwrap().unwrap();
            prop_assert_eq!(
                &decoded,
                original,
                "frame {} mismatch after decoding from combined",
                i
            );
        }
    }

    /// FLAGS の保存性: エンコード/デコードでフラグが保存される
    ///
    /// 数学的意義: フラグビットの保存性
    #[test]
    fn prop_flags_preserved(
        stream_id in valid_stream_id(),
        end_stream in any::<bool>(),
        end_headers in any::<bool>(),
        header_block in arbitrary_bytes(100),
    ) {
        let frame = Frame::Headers(HeadersFrame {
            stream_id,
            end_stream,
            end_headers,
            priority_fields: None,
            header_block_fragment: header_block,
            pad_length: None,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        // フラグフィールドを直接検証 (byte 4)
        let flags_byte = encoded[4];
        let end_stream_bit = (flags_byte & 0x01) != 0;
        let end_headers_bit = (flags_byte & 0x04) != 0;

        prop_assert_eq!(end_stream_bit, end_stream, "END_STREAM flag mismatch");
        prop_assert_eq!(end_headers_bit, end_headers, "END_HEADERS flag mismatch");

        // デコード後も一致
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Headers(hf) = decoded {
            prop_assert_eq!(hf.end_stream, end_stream);
            prop_assert_eq!(hf.end_headers, end_headers);
        } else {
            panic!("expected HEADERS frame");
        }
    }

    /// パディングの整合性: パディング長 + データ長 + 1 = ペイロード長
    ///
    /// 数学的意義: パディング構造の不変条件
    #[test]
    fn prop_padding_structure_invariant(
        stream_id in valid_stream_id(),
        data in arbitrary_bytes(200),
        pad_length in 0..100u8,
    ) {
        let frame = Frame::Data(DataFrame {
            stream_id,
            end_stream: false,
            data: data.clone(),
            pad_length: Some(pad_length),
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        // length フィールド
        let length_field = (u32::from(encoded[0]) << 16)
            | (u32::from(encoded[1]) << 8)
            | u32::from(encoded[2]);

        // PADDED フラグが設定されていることを確認
        let flags = encoded[4];
        prop_assert!((flags & 0x08) != 0, "PADDED flag must be set");

        // ペイロード構造の検証
        // payload = [pad_length: 1 byte] [data: N bytes] [padding: pad_length bytes]
        let expected_payload_len = 1 + data.len() + pad_length as usize;
        prop_assert_eq!(length_field as usize, expected_payload_len);

        // パディング長フィールドの値を検証
        let pad_length_field = encoded[9];  // ペイロードの最初のバイト
        prop_assert_eq!(pad_length_field, pad_length);

        // デコード後のデータが正しいことを確認
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        if let Frame::Data(df) = decoded {
            prop_assert_eq!(df.data, data, "data must be preserved after padding");
            prop_assert_eq!(df.pad_length, Some(pad_length));
        } else {
            panic!("expected DATA frame");
        }
    }

    /// stream_id の範囲制約: エンコード後も 31 ビット範囲内
    ///
    /// 数学的意義: stream_id の範囲保存性
    #[test]
    fn prop_stream_id_31bit_range(
        stream_id in (0..=0x7FFF_FFFFu32).prop_map(StreamId::from_wire),
    ) {
        let frame = Frame::WindowUpdate(WindowUpdateFrame {
            stream_id,
            window_size_increment: 1000,
        });

        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        // stream_id フィールドを抽出 (bytes 5-8)
        // R ビット (最上位ビット) をマスクして取得
        let encoded_stream_id = ((u32::from(encoded[5]) & 0x7f) << 24)
            | (u32::from(encoded[6]) << 16)
            | (u32::from(encoded[7]) << 8)
            | u32::from(encoded[8]);

        prop_assert_eq!(encoded_stream_id, stream_id.as_u32());

        // 最上位ビット (R ビット) は 0
        prop_assert_eq!(encoded[5] & 0x80, 0, "R bit must be 0");
    }

    /// デコーダーの clear 後は初期状態に戻る
    ///
    /// 数学的意義: clear の冪等性と初期状態への復帰
    #[test]
    fn prop_decoder_clear_resets_state(
        partial_data in arbitrary_bytes(50),
    ) {
        let mut decoder = FrameDecoder::new(16384);

        // 部分的なデータを feed
        decoder.feed(&partial_data);
        prop_assert_eq!(decoder.buffered_len(), partial_data.len());

        // clear 実行
        decoder.clear();

        // 初期状態に戻る
        prop_assert_eq!(decoder.buffered_len(), 0);

        // 正常にデコードできる
        let frame = Frame::Ping(PingFrame { ack: false, opaque_data: [1, 2, 3, 4, 5, 6, 7, 8] });
        let mut encoder = FrameEncoder::new();
        encoder.encode(&frame).unwrap();
        let encoded = encoder.take();

        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap();
        prop_assert!(decoded.is_some());
    }
}
