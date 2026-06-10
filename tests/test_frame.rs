//! フレームエンコード/デコードの単体テスト
//!
//! PBT では到達しない意図的なエラーパスと frame_type ラベルの整合性テスト。

use shiguredo_http2::frame::{ContinuationFrame, PriorityUpdateFrame};
use shiguredo_http2::{
    DataFrame, ErrorCode, Frame, FrameDecoder, FrameEncoder, GoawayFrame, HeadersFrame,
    LastStreamId, NonZeroStreamId, PingFrame, RstStreamFrame, SettingsFrame, WindowIncrement,
    WindowUpdateFrame,
};

/// RFC 9113 §4.1 の 9 バイトフレームヘッダー (length 24-bit + type 8-bit + flags 8-bit +
/// R 1-bit + stream_id 31-bit) + 任意 payload を直接組み立てる。`FrameEncoder` は
/// `stream_id=0` / `increment=0` を型レベルで弾くため、不正値のエラーパス検証用に raw バイト列が必要。
fn build_frame_bytes(frame_type: u8, flags: u8, stream_id: u32, payload: &[u8]) -> Vec<u8> {
    let length = u32::try_from(payload.len()).expect("payload は u32 範囲に収まる");
    let length_bytes = length.to_be_bytes();
    let stream_id_bytes = (stream_id & 0x7FFF_FFFF).to_be_bytes();
    let mut buf = Vec::new();
    buf.extend_from_slice(&length_bytes[1..4]);
    buf.push(frame_type);
    buf.push(flags);
    buf.extend_from_slice(&stream_id_bytes);
    buf.extend_from_slice(payload);
    buf
}

/// `build_frame_bytes` でフレームを構築し、`FrameDecoder` が PROTOCOL_ERROR の接続エラーを返すことを確認する。
fn assert_decode_protocol_error(
    frame_type: u8,
    flags: u8,
    stream_id: u32,
    payload: &[u8],
    context: &str,
) {
    let buf = build_frame_bytes(frame_type, flags, stream_id, payload);
    let mut decoder = FrameDecoder::new(16384);
    decoder.feed(&buf);
    let result = decoder.decode();
    assert!(result.is_err(), "decode はエラーを返すべき: {context}");
    let err = result.expect_err("既に is_err を確認済み");
    assert!(err.is_connection_error(), "接続エラーになるべき: {context}");
    assert_eq!(
        err.error_code(),
        Some(ErrorCode::ProtocolError),
        "エラーコードは PROTOCOL_ERROR であるべき: {context}"
    );
}

/// RFC 9113 §6.1: DATA フレームの stream_id=0 は PROTOCOL_ERROR の接続エラーになる。
///
/// payload 長を「空 / 短 / 長 / 典型値」の 4 ケースで検査する (長さ依存のリグレッションを拾うため)。
#[test]
fn test_data_stream_id_zero_error() {
    for payload_len in [0usize, 1, 64, 100] {
        let payload = vec![0u8; payload_len];
        assert_decode_protocol_error(
            0x00,
            0x00,
            0,
            &payload,
            &format!("DATA フレーム stream_id=0 payload_len={payload_len}"),
        );
    }
}

/// RFC 9113 §6.2: HEADERS フレームの stream_id=0 は PROTOCOL_ERROR の接続エラーになる。
///
/// payload 長を「空 / 短 / 長 / 典型値」の 4 ケースで検査する。
#[test]
fn test_headers_stream_id_zero_error() {
    for payload_len in [0usize, 1, 32, 50] {
        let payload = vec![0u8; payload_len];
        assert_decode_protocol_error(
            0x01,
            0x04, // END_HEADERS flag
            0,
            &payload,
            &format!("HEADERS フレーム stream_id=0 payload_len={payload_len}"),
        );
    }
}

/// RFC 9113 §6.4: RST_STREAM フレームの stream_id=0 は PROTOCOL_ERROR の接続エラーになる。
///
/// RST_STREAM の payload は 4 バイト固定 (error_code) で stream_id=0 の検出と独立のため、固定 1 ケース。
#[test]
fn test_rst_stream_stream_id_zero_error() {
    let payload = 0x12345678u32.to_be_bytes();
    assert_decode_protocol_error(0x03, 0x00, 0, &payload, "RST_STREAM フレーム stream_id=0");
}

/// RFC 9113 §6.10: CONTINUATION フレームの stream_id=0 は PROTOCOL_ERROR の接続エラーになる。
///
/// payload 長を「空 / 短 / 長 / 典型値」の 4 ケースで検査する。
#[test]
fn test_continuation_stream_id_zero_error() {
    for payload_len in [0usize, 1, 32, 50] {
        let payload = vec![0u8; payload_len];
        assert_decode_protocol_error(
            0x09,
            0x04, // END_HEADERS flag
            0,
            &payload,
            &format!("CONTINUATION フレーム stream_id=0 payload_len={payload_len}"),
        );
    }
}

/// RFC 9113 §6.9: WINDOW_UPDATE の increment=0 は PROTOCOL_ERROR になる。
/// 接続フロー制御 (stream_id=0) では接続エラー、ストリームフロー制御 (stream_id != 0)
/// ではストリームエラーとして扱う。
#[test]
fn test_window_update_zero_increment_error() {
    for stream_id in [0u32, 1] {
        let payload = [0u8; 4]; // increment = 0
        let buf = build_frame_bytes(0x08, 0x00, stream_id, &payload);
        let mut decoder = FrameDecoder::new(16384);
        decoder.feed(&buf);
        let result = decoder.decode();
        let context = format!("WINDOW_UPDATE フレーム increment=0 stream_id={stream_id}");
        assert!(result.is_err(), "decode はエラーを返すべき: {context}");
        let err = result.expect_err("既に is_err を確認済み");
        assert_eq!(
            err.error_code(),
            Some(ErrorCode::ProtocolError),
            "エラーコードは PROTOCOL_ERROR であるべき: {context}"
        );
        if stream_id == 0 {
            assert!(
                err.is_connection_error(),
                "接続フロー制御の increment=0 は接続エラーになるべき: {context}"
            );
        } else {
            assert!(
                err.is_stream_error(),
                "ストリームフロー制御の increment=0 はストリームエラーになるべき: {context}"
            );
        }
    }
}

/// RFC 9113 §6.3: PRIORITY フレームの stream_id=0 は PROTOCOL_ERROR の接続エラーになる。
///
/// PRIORITY の payload は 5 バイト固定 (exclusive bit + stream_dependency + weight) で
/// stream_id=0 の検出と独立のため、固定 1 ケース。
#[test]
fn test_priority_stream_id_zero_error() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x12345678u32.to_be_bytes());
    payload.push(16);
    assert_decode_protocol_error(0x02, 0x00, 0, &payload, "PRIORITY フレーム stream_id=0");
}

/// RFC 9113 §4.1: エンコード後の `frame_type` フィールドはフレーム種別と一致する。
///
/// 9 フレーム種別をテーブル駆動で検査する (DATA / HEADERS / RST_STREAM / SETTINGS / PING /
/// GOAWAY / WINDOW_UPDATE / CONTINUATION / PRIORITY_UPDATE)。
#[test]
fn test_encoded_frame_type_correct() {
    let stream_id = NonZeroStreamId::from_static(1);
    let data = vec![0u8; 8];
    let increment = WindowIncrement::from_static(1);
    let last_stream_id = LastStreamId::from_static(0);

    let cases: Vec<(Frame, u8)> = vec![
        (
            Frame::Data(DataFrame {
                stream_id,
                end_stream: false,
                data: data.clone(),
                pad_length: None,
            }),
            0x00,
        ),
        (
            Frame::Headers(HeadersFrame {
                stream_id,
                end_stream: false,
                end_headers: true,
                priority_fields: None,
                header_block_fragment: data.clone(),
                pad_length: None,
            }),
            0x01,
        ),
        (
            Frame::RstStream(RstStreamFrame {
                stream_id,
                error_code: 0,
            }),
            0x03,
        ),
        (Frame::Settings(SettingsFrame::new()), 0x04),
        (
            Frame::Ping(PingFrame {
                ack: false,
                opaque_data: [0u8; 8],
            }),
            0x06,
        ),
        (
            Frame::Goaway(GoawayFrame {
                last_stream_id,
                error_code: 0,
                debug_data: vec![],
            }),
            0x07,
        ),
        (
            Frame::WindowUpdate(WindowUpdateFrame::for_stream(stream_id, increment)),
            0x08,
        ),
        (
            Frame::Continuation(ContinuationFrame {
                stream_id,
                end_headers: true,
                header_block_fragment: data,
            }),
            0x09,
        ),
        (
            Frame::PriorityUpdate(PriorityUpdateFrame {
                prioritized_element_id: stream_id,
                priority_field_value: vec![],
            }),
            0x10,
        ),
    ];

    for (frame, expected_type) in cases {
        let mut encoder = FrameEncoder::new();
        encoder
            .encode(&frame)
            .expect("有効なフレームはエンコードできる");
        let encoded = encoder.buffer();
        // フレームヘッダー byte 3 が frame_type フィールド
        assert_eq!(
            encoded[3],
            expected_type,
            "frame_type フィールドが期待値と一致しない: {:?}",
            frame.frame_type()
        );
    }
}

/// `FrameDecoder::clear` 後はバッファが空になり、新規入力を正常にデコードできる。
///
/// 代表値として 50 バイトの partial データの 1 ケースで検査する。
#[test]
fn test_decoder_clear_resets_state() {
    let mut decoder = FrameDecoder::new(16384);

    // 部分的なデータを feed
    let partial_data = vec![0xABu8; 50];
    decoder.feed(&partial_data);
    assert_eq!(decoder.buffered_len(), partial_data.len());

    // clear で初期状態に戻る
    decoder.clear();
    assert_eq!(decoder.buffered_len(), 0);

    // clear 後に正常な PING フレームを feed してデコード成功を確認する
    let frame = Frame::Ping(PingFrame {
        ack: false,
        opaque_data: [1, 2, 3, 4, 5, 6, 7, 8],
    });
    let mut encoder = FrameEncoder::new();
    encoder
        .encode(&frame)
        .expect("PING フレームはエンコードできる");
    let encoded = encoder.buffer().to_vec();

    decoder.feed(&encoded);
    let decoded = decoder
        .decode()
        .expect("clear 後に新規入力をデコードできる");
    assert!(decoded.is_some(), "PING フレームをデコードできる");
}
