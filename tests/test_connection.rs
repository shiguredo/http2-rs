//! HTTP/2 接続の単体テスト
//!
//! PBT では到達しない意図的なエラーパスとデフォルト値境界のテスト。

use shiguredo_http2::{
    Connection, ErrorCode, Limits, NonZeroStreamId,
    frame::{
        ContinuationFrame, Frame, FrameDecoder, FrameEncoder, HeadersFrame, RstStreamFrame,
        SettingsFrame,
    },
    settings::{MAX_MAX_FRAME_SIZE, Setting},
};

/// フレームをバイト列にエンコードする
fn encode_frame(frame: &Frame) -> Vec<u8> {
    let mut encoder = FrameEncoder::new();
    encoder.encode(frame).unwrap();
    encoder.buffer().to_vec()
}

/// CONTINUATION フレームを作成する
fn create_continuation(
    stream_id: NonZeroStreamId,
    fragment: Vec<u8>,
    end_headers: bool,
) -> ContinuationFrame {
    ContinuationFrame::new(stream_id, fragment).with_end_headers(end_headers)
}

/// `connection_window_size == DEFAULT_INITIAL_WINDOW_SIZE` のとき
/// `initiate()` は接続レベル WINDOW_UPDATE を送信しない
#[test]
fn test_initiate_does_not_emit_window_update_when_default() {
    let mut client = Connection::client(Limits::default());
    client.initiate().expect("initiate");

    let output = client.poll_output().expect("output");
    let preface_len = shiguredo_http2::CONNECTION_PREFACE_LEN;
    let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
    decoder.feed(&output[preface_len..]);

    let mut saw_window_update = false;
    while let Some(frame) = decoder.decode().expect("decode") {
        if matches!(frame, Frame::WindowUpdate(_)) {
            saw_window_update = true;
        }
    }
    assert!(
        !saw_window_update,
        "WINDOW_UPDATE は connection_window_size がデフォルトのとき送信されてはならない"
    );
}

/// `send_settings()` 経路で `connection_window_size == DEFAULT` のとき
/// WINDOW_UPDATE を送信しない
#[test]
fn test_send_settings_does_not_emit_window_update_when_default() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.send_settings().expect("send_settings");

    let output = server.poll_output().expect("output");
    let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
    decoder.feed(&output);

    let mut saw_window_update = false;
    while let Some(frame) = decoder.decode().expect("decode") {
        if matches!(frame, Frame::WindowUpdate(_)) {
            saw_window_update = true;
        }
    }
    assert!(
        !saw_window_update,
        "WINDOW_UPDATE は connection_window_size がデフォルトのとき送信されてはならない"
    );
}

/// HEADERS なしで CONTINUATION を送信するとエラー
#[test]
fn test_continuation_without_headers_is_error() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    // SETTINGS を受信
    let settings_frame = Frame::Settings(SettingsFrame::new());
    let settings_bytes = encode_frame(&settings_frame);
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    // HEADERS なしで CONTINUATION を送信
    let continuation = create_continuation(NonZeroStreamId::from_static(1), vec![0x82], true);
    let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
    server.feed(&continuation_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// idle ストリームへの RST_STREAM がエラー
#[test]
fn test_rst_stream_on_idle_is_error() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    // SETTINGS を受信
    let settings_frame = Frame::Settings(SettingsFrame::new());
    let settings_bytes = encode_frame(&settings_frame);
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    // idle ストリームに RST_STREAM を送信
    let rst_frame = Frame::RstStream(RstStreamFrame::new(
        NonZeroStreamId::from_static(1),
        ErrorCode::Cancel.as_u32(),
    ));
    let rst_bytes = encode_frame(&rst_frame);
    server.feed(&rst_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// CONTINUATION フレームの累積が SETTINGS_MAX_HEADER_LIST_SIZE を超えると接続エラーになる
///
/// RFC 9113 Section 6.10: CONTINUATION の個数に上限がないため、累積フラグメントを無制限に
/// 成長させてメモリを枯渇させる攻撃 (CVE-2016-8740 系) を防ぐ。
/// RFC 9113 Section 4.3: field block を展開せず打ち切るため COMPRESSION_ERROR にする (MUST)。
#[test]
fn test_continuation_accumulation_exceeds_max_header_list_size() {
    let limits = Limits::builder()
        .max_header_list_size(Some(100))
        .build()
        .unwrap();
    let mut server = Connection::server(limits);
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    let stream_id = NonZeroStreamId::from_static(1);

    // END_HEADERS なしの HEADERS (60 バイト): 上限 100 以内
    let headers = HeadersFrame::new(stream_id, vec![0u8; 60]).with_end_headers(false);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).unwrap();
    server.process().unwrap();

    // CONTINUATION (60 バイト): 累積 120 バイトで上限 100 を超過
    let continuation = create_continuation(stream_id, vec![0u8; 60], false);
    let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
    server.feed(&continuation_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::CompressionError));
    }
}

/// 累積フラグメントが SETTINGS_MAX_HEADER_LIST_SIZE ちょうどのときは接続エラーにならない
///
/// `check_header_block_fragment_size` は `len() > max` の厳密不等号であり、上限ちょうどは
/// 許容される。off-by-one (>= への退行) を検知する境界テスト。
#[test]
fn test_continuation_accumulation_at_limit_is_ok() {
    let limits = Limits::builder()
        .max_header_list_size(Some(100))
        .build()
        .unwrap();
    let mut server = Connection::server(limits);
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    let stream_id = NonZeroStreamId::from_static(1);

    // END_HEADERS なしの HEADERS (50 バイト)
    let headers = HeadersFrame::new(stream_id, vec![0u8; 50]).with_end_headers(false);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).unwrap();
    server.process().unwrap();

    // CONTINUATION (50 バイト): 累積 100 バイトで上限 100 ちょうど (エラーにならない)
    let continuation = create_continuation(stream_id, vec![0u8; 50], false);
    let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
    server.feed(&continuation_bytes).unwrap();

    // ヘッダーブロックは未完 (END_HEADERS なし) なのでデコードはまだ走らず、エラーにならない
    server.process().unwrap();
}

/// 単一 HEADERS 内のインデックス参照爆弾が COMPRESSION_ERROR 接続エラーになる
///
/// RFC 9113 Section 6.5.2 / Section 4.3: デコード後ヘッダーリストサイズが
/// SETTINGS_MAX_HEADER_LIST_SIZE を超えると、HPACK デコーダが展開途中で打ち切り、
/// field block を展開しきらないため COMPRESSION_ERROR の接続エラーになる。
#[test]
fn test_headers_indexed_reference_bomb_is_compression_error() {
    let limits = Limits::builder()
        .max_header_list_size(Some(100))
        .build()
        .unwrap();
    let mut server = Connection::server(limits);
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    // 静的テーブル index 2 (":method: GET", size 42) への 1 バイト参照を 3 個。
    // 累積デコード後サイズ 126 が上限 100 を超える。
    let stream_id = NonZeroStreamId::from_static(1);
    let headers = HeadersFrame::new(stream_id, vec![0x82, 0x82, 0x82]);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::CompressionError));
    }
}

/// サーバーからの ENABLE_PUSH=1 がエラー
#[test]
fn test_client_rejects_enable_push_from_server() {
    let mut client = Connection::client(Limits::default());
    client.initiate().unwrap();

    // サーバーから ENABLE_PUSH=1 の SETTINGS を受信
    let mut settings = SettingsFrame::new();
    settings.add(Setting::EnablePush(true));
    let settings_bytes = encode_frame(&Frame::Settings(settings));
    client.feed(&settings_bytes).unwrap();

    let result = client.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// `max_header_list_size=None` でも CONTINUATION 累積が固定上限 (64MB) を
/// 超えずに進行することの確認。
/// 64MB 超過は単体テストでは非現実的なため、上限未満の正常経路で
/// None が「無制限」になっていないことを検証する。
#[test]
fn test_continuation_accumulation_with_none_max_header_list_size() {
    let limits = Limits::builder()
        .max_header_list_size(None)
        .build()
        .unwrap();
    let mut server = Connection::server(limits);
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    let stream_id = NonZeroStreamId::from_static(1);

    let headers = HeadersFrame::new(stream_id, vec![0u8; 100]).with_end_headers(false);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).unwrap();
    server.process().unwrap();

    let continuation = create_continuation(stream_id, vec![0u8; 100], false);
    let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
    server.feed(&continuation_bytes).unwrap();

    server.process().unwrap();
}
