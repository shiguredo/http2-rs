//! HEADERS 関連の PBT
//!
//! Continuation フレーム、HPACK エンコード、疑似ヘッダー検証を含む。

use proptest::prelude::*;
use shiguredo_http2::{
    Connection, ErrorCode, HeaderField, HpackEncoder, Limits, NonZeroStreamId,
    frame::{ContinuationFrame, Frame, HeadersFrame, SettingsFrame},
};

use super::{client_stream_id, encode_frame};

/// HEADERS フレームを作成する（END_HEADERS なし）
fn create_headers_without_end_headers(
    stream_id: NonZeroStreamId,
    fragment: Vec<u8>,
) -> HeadersFrame {
    HeadersFrame::new(stream_id, fragment)
        .with_end_stream(false)
        .with_end_headers(false)
}

/// 有効なリクエストヘッダーを HPACK エンコードする
/// (:method GET, :scheme https, :path /, :authority example.com)
pub(super) fn encode_valid_request_headers() -> Vec<u8> {
    let mut encoder = HpackEncoder::new(4096);
    let headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "example.com").unwrap(),
    ];
    let mut buf = Vec::new();
    encoder.encode(&mut buf, &headers);
    buf
}

/// CONTINUATION フレームを作成する
fn create_continuation(
    stream_id: NonZeroStreamId,
    fragment: Vec<u8>,
    end_headers: bool,
) -> ContinuationFrame {
    ContinuationFrame::new(stream_id, fragment).with_end_headers(end_headers)
}

proptest! {
    /// CONTINUATION フレームが先行する HEADERS なしで受信された場合、PROTOCOL_ERROR
    #[test]
    fn prop_continuation_without_headers_is_error(stream_id in client_stream_id()) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信して接続をアクティブにする
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // HEADERS なしで CONTINUATION を送信
        let continuation = create_continuation(stream_id, vec![0x82], true);
        let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
        server.feed(&continuation_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// CONTINUATION フレームのストリーム ID が一致しない場合、PROTOCOL_ERROR
    #[test]
    fn prop_continuation_stream_id_mismatch_is_error(
        first_id in client_stream_id(),
        second_id in client_stream_id(),
    ) {
        prop_assume!(first_id != second_id);

        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // HEADERS (END_HEADERS なし)
        let headers = create_headers_without_end_headers(first_id, vec![0x82]);
        let headers_bytes = encode_frame(&Frame::Headers(headers));
        server.feed(&headers_bytes).unwrap();
        server.process().unwrap();

        // 異なるストリーム ID で CONTINUATION を送信
        let continuation = create_continuation(second_id, vec![0x84], true);
        let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
        server.feed(&continuation_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// 初回 HEADERS に疑似ヘッダーが無い場合は PROTOCOL_ERROR
    ///
    /// RFC 9113 Section 8.1 / 8.3.1: 初回 HEADERS には疑似ヘッダーが必須
    #[test]
    fn prop_initial_headers_without_pseudo_is_error(
        stream_id in client_stream_id(),
    ) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信して接続をアクティブにする
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // 疑似ヘッダーなしのヘッダーブロックを HPACK エンコード
        let headers = vec![
            HeaderField::new("content-type", "text/html").unwrap(),
        ];
        let mut encoder = HpackEncoder::new(4096);
        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        let headers_frame = HeadersFrame::new(stream_id, encoded)
            .with_end_stream(true)
            .with_end_headers(true);
        let headers_bytes = encode_frame(&Frame::Headers(headers_frame));
        server.feed(&headers_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// 不正 HPACK データで COMPRESSION_ERROR になるテスト
    ///
    /// RFC 9113 Section 4.3: HPACK デコード失敗は COMPRESSION_ERROR
    #[test]
    fn prop_invalid_hpack_causes_compression_error(
        stream_id in client_stream_id(),
    ) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信して接続をアクティブにする
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // 不正な HPACK データを含む HEADERS フレーム
        // 0xFF はインデックス 127 以上を示すが、後続データが不足しているため不正
        let invalid_hpack = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let headers_frame = HeadersFrame::new(stream_id, invalid_hpack)
            .with_end_stream(true)
            .with_end_headers(true);
        let headers_bytes = encode_frame(&Frame::Headers(headers_frame));
        server.feed(&headers_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::CompressionError));
        }
    }
}
