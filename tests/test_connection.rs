//! HTTP/2 接続の単体テスト
//!
//! PBT では到達しない意図的なエラーパスとデフォルト値境界のテスト。

use shiguredo_http2::{
    Connection, ErrorCode, Event, HeaderField, HpackEncoder, LastStreamId, Limits, NonZeroStreamId,
    WindowIncrement,
    frame::{
        ContinuationFrame, DataFrame, Frame, FrameDecoder, FrameEncoder, GoawayFrame, HeadersFrame,
        PingFrame, RstStreamFrame, SettingsFrame, WindowUpdateFrame,
    },
    settings::{MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, Setting},
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

/// 有効なリクエストヘッダー (:method GET, :scheme https, :path /, :authority example.com) を
/// HPACK エンコードする。ストリーム ID 違反 (偶数 / 非単調) のテストで再利用する最小セット。
fn encode_valid_request_headers() -> Vec<u8> {
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
///
/// RFC 9113 Section 6.10: 先行する HEADERS/PUSH_PROMISE/CONTINUATION の無い
/// CONTINUATION は PROTOCOL_ERROR の接続エラーにしなければならない (MUST)。
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
///
/// RFC 9113 Section 6.4: idle ストリームを指す RST_STREAM の受信は
/// PROTOCOL_ERROR の接続エラーとして扱わなければならない (MUST)。
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
///
/// RFC 9113 Section 6.5.2: クライアントは SETTINGS_ENABLE_PUSH=1 の受信を
/// PROTOCOL_ERROR の接続エラーとして扱わなければならない (MUST)。
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

/// RFC 9113 Section 5.1: idle ストリームへの DATA は PROTOCOL_ERROR の接続エラーになる。
#[test]
fn test_data_on_idle_stream_is_error() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    // SETTINGS を受信して接続をアクティブにする
    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    // idle ストリーム (stream_id=1) に DATA を送信
    let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
        NonZeroStreamId::from_static(1),
        vec![1, 2, 3],
    )));
    server.feed(&data_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// RFC 9113 §8.3.1 + §8.1.1: 必須擬似ヘッダーを欠いたリクエストは malformed であり、
/// PROTOCOL_ERROR (§8.1.1 では malformed はストリームエラーとして扱う MUST、実装上は
/// HPACK デコードの段階で接続エラーに昇格する場合がある) で拒否される。
#[test]
fn test_initial_headers_without_pseudo_is_error() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    // 擬似ヘッダーなしのヘッダーブロックを HPACK エンコード
    let headers = vec![HeaderField::new("content-type", "text/html").unwrap()];
    let mut encoder = HpackEncoder::new(4096);
    let mut encoded = Vec::new();
    encoder.encode(&mut encoded, &headers);

    let headers_frame = HeadersFrame::new(NonZeroStreamId::from_static(1), encoded)
        .with_end_stream(true)
        .with_end_headers(true);
    let headers_bytes = encode_frame(&Frame::Headers(headers_frame));
    server.feed(&headers_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// RFC 9113 Section 4.3: 不正な HPACK データは COMPRESSION_ERROR の接続エラーになる。
#[test]
fn test_invalid_hpack_causes_compression_error() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    // 0xFF はインデックス 127 以上を示すが、後続データが不足しているため不正
    let invalid_hpack = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let headers_frame = HeadersFrame::new(NonZeroStreamId::from_static(1), invalid_hpack)
        .with_end_stream(true)
        .with_end_headers(true);
    let headers_bytes = encode_frame(&Frame::Headers(headers_frame));
    server.feed(&headers_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::CompressionError));
    }
}

/// RFC 9113 Section 5.1.1: クライアント開始のストリームは奇数 ID でなければならず、
/// サーバーが偶数 ID の新規ストリームを受信した場合は PROTOCOL_ERROR の接続エラーになる。
#[test]
fn test_server_rejects_even_stream_id() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    // 偶数ストリーム ID で HEADERS を送信
    let headers = HeadersFrame::new(
        NonZeroStreamId::from_static(2),
        encode_valid_request_headers(),
    )
    .with_end_stream(true)
    .with_end_headers(true);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// RFC 9113 Section 5.1.1: 新規ストリーム ID は既存のすべてより大きくなければならず、
/// 違反は PROTOCOL_ERROR の接続エラーになる。
#[test]
fn test_non_monotonic_stream_id_is_error() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    // 最初のストリーム (奇数 ID = 5)
    let first_id = NonZeroStreamId::from_static(5);
    let headers1 = HeadersFrame::new(first_id, encode_valid_request_headers())
        .with_end_stream(true)
        .with_end_headers(true);
    let headers1_bytes = encode_frame(&Frame::Headers(headers1));
    server.feed(&headers1_bytes).unwrap();
    server.process().unwrap();

    // 小さいストリーム ID (3) で新しいストリームを開始 → 単調増加違反
    let second_id = NonZeroStreamId::from_static(3);
    let headers2 = HeadersFrame::new(second_id, encode_valid_request_headers())
        .with_end_stream(true)
        .with_end_headers(true);
    let headers2_bytes = encode_frame(&Frame::Headers(headers2));
    server.feed(&headers2_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// RFC 9113 Section 5.1: idle ストリームへの WINDOW_UPDATE は PROTOCOL_ERROR の接続エラーになる。
#[test]
fn test_window_update_on_idle_stream_is_error() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    let wu_frame = Frame::WindowUpdate(WindowUpdateFrame::for_stream(
        NonZeroStreamId::from_static(1),
        WindowIncrement::from_static(1000),
    ));
    let wu_bytes = encode_frame(&wu_frame);
    server.feed(&wu_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// RFC 9113 Section 6.8: GOAWAY 受信後の新規ストリーム開始は PROTOCOL_ERROR の接続エラーになる。
#[test]
fn test_start_stream_after_goaway_is_error() {
    let mut client = Connection::client(Limits::default());
    client.initiate().unwrap();

    // サーバーから SETTINGS を受信
    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    client.feed(&settings_bytes).unwrap();
    client.process().unwrap();

    // サーバーから GOAWAY を受信
    let goaway = Frame::Goaway(GoawayFrame::new(
        LastStreamId::from_static(0),
        ErrorCode::NoError.as_u32(),
    ));
    let goaway_bytes = encode_frame(&goaway);
    client.feed(&goaway_bytes).unwrap();
    client.process().unwrap();

    // GOAWAY 後に新規ストリームを開始しようとする
    let headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":authority", "example.com").unwrap(),
    ];
    let result = client.start_stream(headers, true);
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// RFC 9113 Section 3.4: サーバーは client preface を受信済みでなければならず、
/// preface 未受信時のフレーム入力は PROTOCOL_ERROR の接続エラーになる。
#[test]
fn test_server_rejects_frame_without_preface() {
    let mut server = Connection::server(Limits::default());
    // mark_preface_received() を呼ばずに initiate
    server.initiate().unwrap();

    // SETTINGS フレームを送信しても preface と一致しないためエラー
    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    let result = server.feed(&settings_bytes);
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// RFC 9113 Section 3.4: mark_preface_received() 後は SETTINGS 処理が成功する。
#[test]
fn test_server_accepts_frame_after_preface() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();

    assert!(server.process().is_ok());
}

/// RFC 9113 Section 8.4: サーバープッシュ非サポートのためサーバーは新規ストリームを開始できない。
#[test]
fn test_server_cannot_start_stream() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).unwrap();
    server.process().unwrap();

    let headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":authority", "example.com").unwrap(),
    ];
    assert!(server.start_stream(headers, true).is_err());
}

/// RFC 9113 Section 6.8: GOAWAY 受信時に GoawayReceived イベントが発火する。
///
/// クライアント単体に SETTINGS を擬似的に feed して Active 状態に遷移させたうえで、
/// GOAWAY 受信時に GoawayReceived イベントが発火することを確認する。
#[test]
fn test_goaway_graceful_shutdown() {
    let mut client = Connection::client(Limits::default());
    client.initiate().unwrap();

    // サーバーからの SETTINGS を擬似的に feed して Active 状態に遷移させる
    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    client.feed(&settings_bytes).unwrap();
    client.process().unwrap();
    // 後段の GoawayReceived 検出のため、SETTINGS 受信時の SettingsReceived を先に消費する
    while client.poll_event().is_some() {}

    // サーバーから GOAWAY を受信
    let goaway = Frame::Goaway(GoawayFrame::new(
        LastStreamId::from_static(0),
        ErrorCode::NoError.as_u32(),
    ));
    let goaway_bytes = encode_frame(&goaway);
    client.feed(&goaway_bytes).unwrap();
    client.process().unwrap();

    // GoawayReceived イベントの発火を確認
    let mut found_goaway = false;
    while let Some(event) = client.poll_event() {
        if matches!(
            event,
            Event::GoawayReceived {
                error_code: ErrorCode::NoError,
                ..
            }
        ) {
            found_goaway = true;
            break;
        }
    }
    assert!(found_goaway, "GoawayReceived イベントが発火するべき");
}

/// RFC 9113 Section 3.4: クライアントからの最初のフレームが SETTINGS でない場合は
/// PROTOCOL_ERROR の接続エラーになる。
#[test]
fn test_first_frame_must_be_settings() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().unwrap();

    // SETTINGS ではなく PING を最初に送信
    let ping_bytes = encode_frame(&Frame::Ping(PingFrame::new([0u8; 8])));
    server.feed(&ping_bytes).unwrap();

    let result = server.process();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.is_connection_error());
        assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
    }
}

/// RFC 9113 Section 6.5.2: INITIAL_WINDOW_SIZE が範囲外の SETTINGS は FLOW_CONTROL_ERROR の
/// 接続エラーになる。境界値 (MAX + 1) と上限 (u32::MAX) の 2 ケースを検査する。
#[test]
fn test_invalid_initial_window_size_is_flow_control_error() {
    for invalid_size in [MAX_INITIAL_WINDOW_SIZE + 1, u32::MAX] {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // decoder が Setting::from_wire で検証するため、raw バイト列を直接構築する
        let mut settings_bytes = Vec::new();
        // フレームヘッダー: length=6, type=0x04 (SETTINGS), flags=0, stream_id=0
        settings_bytes.extend_from_slice(&[0x00, 0x00, 0x06, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
        // SETTINGS パラメータ: id=0x0004 (INITIAL_WINDOW_SIZE), value=invalid_size
        settings_bytes.extend_from_slice(&0x0004u16.to_be_bytes());
        settings_bytes.extend_from_slice(&invalid_size.to_be_bytes());
        client.feed(&settings_bytes).unwrap();

        let result = client.process();
        assert!(
            result.is_err(),
            "範囲外の INITIAL_WINDOW_SIZE は FLOW_CONTROL_ERROR になるべき: invalid_size={invalid_size}"
        );
        if let Err(e) = result {
            assert!(e.is_connection_error());
            assert_eq!(e.error_code(), Some(ErrorCode::FlowControlError));
        }
    }
}
