//! HTTP/2 接続の単体テスト
//!
//! PBT では到達しない意図的なエラーパスとデフォルト値境界のテスト。

use shiguredo_http2::{
    Connection, ErrorCode, Event, HeaderField, HpackEncoder, LastStreamId, Limits, NonZeroStreamId,
    WindowIncrement, WindowSize,
    frame::{
        ContinuationFrame, DataFrame, Frame, FrameDecoder, FrameEncoder, GoawayFrame, HeadersFrame,
        PingFrame, RstStreamFrame, SettingsFrame, WindowUpdateFrame,
    },
    settings::{MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, Setting},
};

/// フレームをバイト列にエンコードする
fn encode_frame(frame: &Frame) -> Vec<u8> {
    let mut encoder = FrameEncoder::new();
    encoder.encode(frame).expect("encode should succeed");
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
    let mut buf = Vec::new();
    encoder.encode(&mut buf, &request_headers());
    buf
}

/// 有効なリクエストヘッダー (生の HeaderField) を生成する
///
/// `start_stream` に渡すために使用する。
fn request_headers() -> Vec<HeaderField> {
    vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "example.com").expect("valid header field"),
    ]
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
    server.initiate().expect("initiate should succeed");

    // SETTINGS を受信
    let settings_frame = Frame::Settings(SettingsFrame::new());
    let settings_bytes = encode_frame(&settings_frame);
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // HEADERS なしで CONTINUATION を送信
    let continuation = create_continuation(NonZeroStreamId::from_static(1), vec![0x82], true);
    let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
    server
        .feed(&continuation_bytes)
        .expect("feed should succeed");

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
    server.initiate().expect("initiate should succeed");

    // SETTINGS を受信
    let settings_frame = Frame::Settings(SettingsFrame::new());
    let settings_bytes = encode_frame(&settings_frame);
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // idle ストリームに RST_STREAM を送信
    let rst_frame = Frame::RstStream(RstStreamFrame::new(
        NonZeroStreamId::from_static(1),
        ErrorCode::Cancel.as_u32(),
    ));
    let rst_bytes = encode_frame(&rst_frame);
    server.feed(&rst_bytes).expect("feed should succeed");

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
        .expect("should succeed");
    let mut server = Connection::server(limits);
    server.mark_preface_received();
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    let stream_id = NonZeroStreamId::from_static(1);

    // END_HEADERS なしの HEADERS (60 バイト): 上限 100 以内
    let headers = HeadersFrame::new(stream_id, vec![0u8; 60]).with_end_headers(false);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // CONTINUATION (60 バイト): 累積 120 バイトで上限 100 を超過
    let continuation = create_continuation(stream_id, vec![0u8; 60], false);
    let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
    server
        .feed(&continuation_bytes)
        .expect("feed should succeed");

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
        .expect("should succeed");
    let mut server = Connection::server(limits);
    server.mark_preface_received();
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    let stream_id = NonZeroStreamId::from_static(1);

    // END_HEADERS なしの HEADERS (50 バイト)
    let headers = HeadersFrame::new(stream_id, vec![0u8; 50]).with_end_headers(false);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // CONTINUATION (50 バイト): 累積 100 バイトで上限 100 ちょうど (エラーにならない)
    let continuation = create_continuation(stream_id, vec![0u8; 50], false);
    let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
    server
        .feed(&continuation_bytes)
        .expect("feed should succeed");

    // ヘッダーブロックは未完 (END_HEADERS なし) なのでデコードはまだ走らず、エラーにならない
    server.process().expect("process should succeed");
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
        .expect("should succeed");
    let mut server = Connection::server(limits);
    server.mark_preface_received();
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // 静的テーブル index 2 (":method: GET", size 42) への 1 バイト参照を 3 個。
    // 累積デコード後サイズ 126 が上限 100 を超える。
    let stream_id = NonZeroStreamId::from_static(1);
    let headers = HeadersFrame::new(stream_id, vec![0x82, 0x82, 0x82]);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).expect("feed should succeed");

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
    client.initiate().expect("initiate should succeed");

    // サーバーから ENABLE_PUSH=1 の SETTINGS を受信
    let mut settings = SettingsFrame::new();
    settings.add(Setting::EnablePush(true));
    let settings_bytes = encode_frame(&Frame::Settings(settings));
    client.feed(&settings_bytes).expect("feed should succeed");

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
        .expect("should succeed");
    let mut server = Connection::server(limits);
    server.mark_preface_received();
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    let stream_id = NonZeroStreamId::from_static(1);

    let headers = HeadersFrame::new(stream_id, vec![0u8; 100]).with_end_headers(false);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    let continuation = create_continuation(stream_id, vec![0u8; 100], false);
    let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
    server
        .feed(&continuation_bytes)
        .expect("feed should succeed");

    server.process().expect("process should succeed");
}

/// RFC 9113 Section 5.1: idle ストリームへの DATA は PROTOCOL_ERROR の接続エラーになる。
#[test]
fn test_data_on_idle_stream_is_error() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().expect("initiate should succeed");

    // SETTINGS を受信して接続をアクティブにする
    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // idle ストリーム (stream_id=1) に DATA を送信
    let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
        NonZeroStreamId::from_static(1),
        vec![1, 2, 3],
    )));
    server.feed(&data_bytes).expect("feed should succeed");

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
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // 擬似ヘッダーなしのヘッダーブロックを HPACK エンコード
    let headers = vec![HeaderField::new("content-type", "text/html").expect("valid header field")];
    let mut encoder = HpackEncoder::new(4096);
    let mut encoded = Vec::new();
    encoder.encode(&mut encoded, &headers);

    let headers_frame = HeadersFrame::new(NonZeroStreamId::from_static(1), encoded)
        .with_end_stream(true)
        .with_end_headers(true);
    let headers_bytes = encode_frame(&Frame::Headers(headers_frame));
    server.feed(&headers_bytes).expect("feed should succeed");

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
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // 0xFF はインデックス 127 以上を示すが、後続データが不足しているため不正
    let invalid_hpack = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let headers_frame = HeadersFrame::new(NonZeroStreamId::from_static(1), invalid_hpack)
        .with_end_stream(true)
        .with_end_headers(true);
    let headers_bytes = encode_frame(&Frame::Headers(headers_frame));
    server.feed(&headers_bytes).expect("feed should succeed");

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
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // 偶数ストリーム ID で HEADERS を送信
    let headers = HeadersFrame::new(
        NonZeroStreamId::from_static(2),
        encode_valid_request_headers(),
    )
    .with_end_stream(true)
    .with_end_headers(true);
    let headers_bytes = encode_frame(&Frame::Headers(headers));
    server.feed(&headers_bytes).expect("feed should succeed");

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
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // 最初のストリーム (奇数 ID = 5)
    let first_id = NonZeroStreamId::from_static(5);
    let headers1 = HeadersFrame::new(first_id, encode_valid_request_headers())
        .with_end_stream(true)
        .with_end_headers(true);
    let headers1_bytes = encode_frame(&Frame::Headers(headers1));
    server.feed(&headers1_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    // 小さいストリーム ID (3) で新しいストリームを開始 → 単調増加違反
    let second_id = NonZeroStreamId::from_static(3);
    let headers2 = HeadersFrame::new(second_id, encode_valid_request_headers())
        .with_end_stream(true)
        .with_end_headers(true);
    let headers2_bytes = encode_frame(&Frame::Headers(headers2));
    server.feed(&headers2_bytes).expect("feed should succeed");

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
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    let wu_frame = Frame::WindowUpdate(WindowUpdateFrame::for_stream(
        NonZeroStreamId::from_static(1),
        WindowIncrement::from_static(1000),
    ));
    let wu_bytes = encode_frame(&wu_frame);
    server.feed(&wu_bytes).expect("feed should succeed");

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
    client.initiate().expect("initiate should succeed");

    // サーバーから SETTINGS を受信
    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    client.feed(&settings_bytes).expect("feed should succeed");
    client.process().expect("process should succeed");

    // サーバーから GOAWAY を受信
    let goaway = Frame::Goaway(GoawayFrame::new(
        LastStreamId::from_static(0),
        ErrorCode::NoError.as_u32(),
    ));
    let goaway_bytes = encode_frame(&goaway);
    client.feed(&goaway_bytes).expect("feed should succeed");
    client.process().expect("process should succeed");

    // GOAWAY 後に新規ストリームを開始しようとする
    let headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "example.com").expect("valid header field"),
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
    server.initiate().expect("initiate should succeed");

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
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");

    assert!(server.process().is_ok());
}

/// RFC 9113 Section 8.4: サーバープッシュ非サポートのためサーバーは新規ストリームを開始できない。
#[test]
fn test_server_cannot_start_stream() {
    let mut server = Connection::server(Limits::default());
    server.mark_preface_received();
    server.initiate().expect("initiate should succeed");

    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    server.feed(&settings_bytes).expect("feed should succeed");
    server.process().expect("process should succeed");

    let headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "example.com").expect("valid header field"),
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
    client.initiate().expect("initiate should succeed");

    // サーバーからの SETTINGS を擬似的に feed して Active 状態に遷移させる
    let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
    client.feed(&settings_bytes).expect("feed should succeed");
    client.process().expect("process should succeed");
    // 後段の GoawayReceived 検出のため、SETTINGS 受信時の SettingsReceived を先に消費する
    while client.poll_event().is_some() {}

    // サーバーから GOAWAY を受信
    let goaway = Frame::Goaway(GoawayFrame::new(
        LastStreamId::from_static(0),
        ErrorCode::NoError.as_u32(),
    ));
    let goaway_bytes = encode_frame(&goaway);
    client.feed(&goaway_bytes).expect("feed should succeed");
    client.process().expect("process should succeed");

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
    server.initiate().expect("initiate should succeed");

    // SETTINGS ではなく PING を最初に送信
    let ping_bytes = encode_frame(&Frame::Ping(PingFrame::new([0u8; 8])));
    server.feed(&ping_bytes).expect("feed should succeed");

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
        client.initiate().expect("initiate should succeed");

        // decoder が Setting::from_wire で検証するため、raw バイト列を直接構築する
        let mut settings_bytes = Vec::new();
        // フレームヘッダー: length=6, type=0x04 (SETTINGS), flags=0, stream_id=0
        settings_bytes.extend_from_slice(&[0x00, 0x00, 0x06, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
        // SETTINGS パラメータ: id=0x0004 (INITIAL_WINDOW_SIZE), value=invalid_size
        settings_bytes.extend_from_slice(&0x0004u16.to_be_bytes());
        settings_bytes.extend_from_slice(&invalid_size.to_be_bytes());
        client.feed(&settings_bytes).expect("feed should succeed");

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

/// reset_stream の終了イベント通知とクローズ済みストリーム追跡のテスト
///
/// 内部リセット (ストリームエラー処理) と明示リセットで `Event::StreamReset` が通知され、
/// リセット済みストリームへの遅延フレームが idle 誤判定で接続エラーに昇格しないことを
/// 検証する。
mod reset_stream {
    use super::*;

    /// 増分 0 のストリーム向け WINDOW_UPDATE (stream_id=1) を raw バイト列で構築する
    ///
    /// `WindowIncrement` 型は非ゼロを構造的に保証するため、raw バイト列で構築する。
    /// RFC 9113 Section 6.9: 増分 0 の WINDOW_UPDATE はエラーとして扱わなければならない (MUST)。
    fn encode_window_update_zero_increment() -> Vec<u8> {
        let mut wu_bytes = Vec::new();
        // フレームヘッダー: length=4, type=0x08 (WINDOW_UPDATE), flags=0, stream_id=1
        wu_bytes.extend_from_slice(&[0x00, 0x00, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00, 0x01]);
        // WINDOW_UPDATE ペイロード: window_size_increment=0
        wu_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        wu_bytes
    }

    /// サーバー接続を初期化して Active 状態にする
    ///
    /// mark_preface_received + initiate + ピア SETTINGS 受信まで完了する。
    fn setup_server() -> Connection {
        setup_server_with_limits(Limits::default())
    }

    /// 指定した Limits でサーバー接続を初期化して Active 状態にする
    fn setup_server_with_limits(limits: Limits) -> Connection {
        let mut server = Connection::server(limits);
        server.mark_preface_received();
        server.initiate().expect("initiate should succeed");
        let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
        server.feed(&settings_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");
        server
    }

    /// クライアント接続を初期化して Active 状態にする
    ///
    /// initiate + ピア SETTINGS 受信まで完了する。
    fn setup_client() -> Connection {
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate should succeed");
        let settings_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
        client.feed(&settings_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");
        client
    }

    /// サーバー側でクライアント開始ストリームを開く (HEADERS を受信する)
    fn open_stream_on_server(server: &mut Connection, stream_id: u32) {
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(stream_id),
            encode_valid_request_headers(),
        )
        .with_end_headers(true);
        let headers_bytes = encode_frame(&Frame::Headers(headers));
        server.feed(&headers_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");
    }

    /// 有効なレスポンスヘッダー (:status 200) を HPACK エンコードする
    fn encode_valid_response_headers() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![HeaderField::new(":status", "200").expect("valid header field")];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// クライアント開始ストリーム ID を生成する
    fn client_stream_id(id: u32) -> shiguredo_http2::StreamId {
        shiguredo_http2::StreamId::Client(shiguredo_http2::ClientStreamId::from_static(id))
    }

    /// イベントキューから条件に一致するイベントを探す
    fn find_event<F>(conn: &mut Connection, mut pred: F) -> bool
    where
        F: FnMut(&Event) -> bool,
    {
        while let Some(event) = conn.poll_event() {
            if pred(&event) {
                return true;
            }
        }
        false
    }

    /// イベントキューに条件に一致するイベントが無いことを検証する
    fn assert_no_event<F>(conn: &mut Connection, pred: F, message: &str)
    where
        F: FnMut(&Event) -> bool,
    {
        assert!(!find_event(conn, pred), "{}", message);
    }

    /// ストリームレベルのフロー制御違反 (受信ウィンドウ超過の DATA) による内部リセットで
    /// `Event::StreamReset` が通知される
    ///
    /// RFC 9113 Section 6.9.1: フロー制御ウィンドウを超える DATA の送信は MUST NOT であり、
    /// ウィンドウを超過するフレームを受け入れられない受信者は FLOW_CONTROL_ERROR の
    /// ストリームエラーで応答してよい (RFC 9113 Section 6.9 の MAY)。
    /// 本実装は RST_STREAM(FLOW_CONTROL_ERROR) を送信し、内部リセット時にも受信パス
    /// (ピアからの RST_STREAM) と対称に `Event::StreamReset` を通知する。
    #[test]
    fn test_flow_control_violation_pushes_stream_reset() {
        // ストリームレベルの受信ウィンドウを 4096 に縮小し、
        // 接続ウィンドウ (65535) は超過しない DATA で違反を起こす
        let limits = Limits::builder()
            .initial_window_size(WindowSize::from_static(4096))
            .build()
            .expect("should succeed");
        let mut server = setup_server_with_limits(limits);
        open_stream_on_server(&mut server, 1);

        // 受信ウィンドウ 4096 を超過する DATA (4097 バイト)
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 4097],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        // Event::StreamReset (FLOW_CONTROL_ERROR) が通知されている
        assert!(
            find_event(&mut server, |e| matches!(
                e,
                Event::StreamReset {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    error_code: ErrorCode::FlowControlError,
                }
            )),
            "Event::StreamReset が通知されるべき"
        );

        // 違反した DATA は DataReceived イベントにならない (RST_STREAM で早期 return される)
        assert_no_event(
            &mut server,
            |e| matches!(e, Event::DataReceived { .. }),
            "フロー制御違反の DATA は DataReceived イベントになってはならない",
        );
    }

    /// ストリーム向け WINDOW_UPDATE の増分 0 (デコードエラー) による内部リセットで
    /// `Event::StreamReset` が通知される
    ///
    /// RFC 9113 Section 6.9: 増分 0 の WINDOW_UPDATE はストリームエラー
    /// (PROTOCOL_ERROR) として扱わなければならない (MUST)。ストリームエラーは
    /// RST_STREAM で処理される (RFC 9113 Section 5.4.2)。
    #[test]
    fn test_decode_error_pushes_stream_reset() {
        let mut server = setup_server();
        open_stream_on_server(&mut server, 1);

        server
            .feed(&encode_window_update_zero_increment())
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        // Event::StreamReset (PROTOCOL_ERROR) が通知されている
        assert!(
            find_event(&mut server, |e| matches!(
                e,
                Event::StreamReset {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    error_code: ErrorCode::ProtocolError,
                }
            )),
            "Event::StreamReset が通知されるべき"
        );
    }

    /// ストリーム向け WINDOW_UPDATE のウィンドウオーバーフローによる内部リセットで
    /// `Event::StreamReset` が通知される
    ///
    /// RFC 9113 Section 6.9.1: フロー制御ウィンドウは 2^31-1 を超えてはならない (MUST NOT)。
    /// 超過は FLOW_CONTROL_ERROR のストリームエラーになり、RST_STREAM が送信される。
    #[test]
    fn test_window_overflow_pushes_stream_reset() {
        let mut server = setup_server();
        open_stream_on_server(&mut server, 1);

        // 送信ウィンドウ (65535) に 2^31-1 を加算すると上限 (2^31-1) を超える
        let wu_frame = Frame::WindowUpdate(WindowUpdateFrame::for_stream(
            NonZeroStreamId::from_static(1),
            WindowIncrement::from_static(WindowIncrement::MAX),
        ));
        let wu_bytes = encode_frame(&wu_frame);
        server.feed(&wu_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        // Event::StreamReset (FLOW_CONTROL_ERROR) が通知されている
        assert!(
            find_event(&mut server, |e| matches!(
                e,
                Event::StreamReset {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    error_code: ErrorCode::FlowControlError,
                }
            )),
            "Event::StreamReset が通知されるべき"
        );
    }

    /// 利用者が `reset_stream` を明示的に呼んだ場合に `Event::StreamReset` が通知される
    ///
    /// 受信パス (ピアからの RST_STREAM) と対称に、送信パスでもストリームの終了を
    /// 利用者が認識できるようにする。RST_STREAM 送信は常に行い、ストリームが
    /// `streams` に存在する場合のみ終了イベントを push して削除するため、
    /// `Event::StreamClosed` は通知されない。
    #[test]
    fn test_explicit_reset_stream_pushes_event() {
        let mut server = setup_server();
        open_stream_on_server(&mut server, 1);

        server
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");

        // Event::StreamReset (CANCEL) が通知されている
        assert!(
            find_event(&mut server, |e| matches!(
                e,
                Event::StreamReset {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    error_code: ErrorCode::Cancel,
                }
            )),
            "Event::StreamReset が通知されるべき"
        );

        // RST_STREAM フレームが出力されている
        let output = server.poll_output().expect("output should exist");
        let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
        decoder.feed(&output);
        let mut found_rst = false;
        while let Some(frame) = decoder.decode().expect("decode should succeed") {
            if matches!(frame, Frame::RstStream(_)) {
                found_rst = true;
            }
        }
        assert!(found_rst, "RST_STREAM フレームが出力されるべき");

        // 内部リセットは Event::StreamClosed を通知しない
        assert_no_event(
            &mut server,
            |e| matches!(e, Event::StreamClosed { .. }),
            "明示リセットで Event::StreamClosed が push されてはならない",
        );
    }

    /// クローズ済みストリームへの明示 `reset_stream` では `Event::StreamReset` が push されない
    ///
    /// RST_STREAM 送信は常に行い、終了イベントの push と `streams` からの削除は
    /// `streams` にストリームが存在する場合のみ行う。
    /// なお、クローズ済みストリームへの RST_STREAM 送信は RFC 9113 Section 5.1 の
    /// 「closed 状態のストリームには PRIORITY 以外を送信してはならない (MUST NOT)」と、
    /// ピアからの RST_STREAM 受信後の送信は Section 5.4.2 の「RST_STREAM への応答で
    /// RST_STREAM を送信してはならない (MUST NOT)」に厳密には抵触しうるが、
    /// 本修正は既存挙動 (streams に存在しないストリームへも送信) を維持する。
    #[test]
    fn test_reset_stream_closed_no_event() {
        // クローズ済みストリーム (ピアからの RST_STREAM で削除済み)
        let mut server = setup_server();
        open_stream_on_server(&mut server, 1);
        let rst_frame = Frame::RstStream(RstStreamFrame::new(
            NonZeroStreamId::from_static(1),
            ErrorCode::Cancel.as_u32(),
        ));
        server
            .feed(&encode_frame(&rst_frame))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        // ピア RST_STREAM 由来の Event::StreamReset を含む既存イベントを消費する
        while server.poll_event().is_some() {}

        server
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");

        assert_no_event(
            &mut server,
            |e| matches!(e, Event::StreamReset { .. }),
            "クローズ済みストリームへの明示リセットで Event::StreamReset が push されてはならない",
        );
    }

    /// idle ストリームへの明示 `reset_stream` では `Event::StreamReset` が push されない
    ///
    /// RST_STREAM 送信は常に行い、終了イベントの push と `streams` からの削除は
    /// `streams` にストリームが存在する場合のみ行う。
    /// なお、idle ストリームへの RST_STREAM 送信は RFC 9113 Section 6.4 の MUST NOT に
    /// 抵触するが、本修正は既存挙動を維持する (このテストは送信の成否を検証しない)。
    #[test]
    fn test_reset_stream_idle_no_event() {
        // idle ストリーム (一度も開かれていない)
        let mut idle_server = setup_server();
        while idle_server.poll_event().is_some() {}
        // 既存挙動 (RST_STREAM 送信のみ) のため戻り値は検証しない。
        // idle ストリームへの明示リセットは別途エラーを返すようになるため、
        // 成功 (Ok) を assert しない。
        let _ = idle_server.reset_stream(client_stream_id(1), ErrorCode::Cancel);

        assert_no_event(
            &mut idle_server,
            |e| matches!(e, Event::StreamReset { .. }),
            "idle ストリームへの明示リセットで Event::StreamReset が push されてはならない",
        );
    }

    /// リセット済みストリームへの遅延 DATA は破棄され、idle 誤判定による接続エラーに
    /// 昇格しない
    ///
    /// クライアントが送信開始したストリーム (last_recv_stream_id 超過) を明示リセットした後に
    /// ピアから DATA が到着しても、`check_not_idle_stream` が `closed_streams` を考慮し、
    /// idle ストリームへのフレームとして接続エラーにしない。
    /// RFC 9113 Section 5.1: RST_STREAM 送信で closed 状態になったストリームへの
    /// 遅延フレームは最小処理して破棄する。
    #[test]
    fn test_reset_stream_delayed_data_discarded() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        client
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");
        // 明示リセット由来の Event::StreamReset を消費する
        while client.poll_event().is_some() {}

        // リセット済みストリームへの遅延 DATA を受信しても接続は維持される
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![1, 2, 3],
        )));
        client.feed(&data_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        // 破棄されたデータは DataReceived イベントにならない
        assert_no_event(
            &mut client,
            |e| matches!(e, Event::DataReceived { .. }),
            "リセット済みストリームへの遅延 DATA は破棄されるべき",
        );
    }

    /// リセット済みストリームへの WINDOW_UPDATE 増分 0 (デコードエラー) が
    /// idle 誤判定による接続エラーに昇格しない
    ///
    /// `Connection::process` のデコードエラー処理は `is_idle_stream` で idle 判定するが、
    /// `closed_streams` を考慮するため、リセット済みストリームへのエラーは
    /// RST_STREAM 送信のみで処理され、接続が維持される。
    /// リセット済みストリーム (streams に存在しない) への呼び出しでは終了イベントは
    /// push されない。
    /// なお、遅延不正フレームに対する 2 本目の RST_STREAM 送信は RFC 9113 Section 5.4.2 の
    /// 複数送信制限 (SHOULD NOT) に厳密には抵触しうるが、本修正は既存挙動を維持する。
    #[test]
    fn test_reset_stream_decode_error_keeps_connection() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        client
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");
        // 明示リセット由来の Event::StreamReset を消費する
        while client.poll_event().is_some() {}

        client
            .feed(&encode_window_update_zero_increment())
            .expect("feed should succeed");

        // デコードエラーが接続エラーに昇格せず、接続が維持される
        client.process().expect("process should succeed");

        // リセット済みストリーム (streams に存在しない) への呼び出しでは
        // Event::StreamReset は push されない
        assert_no_event(
            &mut client,
            |e| matches!(e, Event::StreamReset { .. }),
            "リセット済みストリームへのデコードエラーで Event::StreamReset が push されてはならない",
        );
    }

    /// GOAWAY 送信後にリセット済みストリームへ遅延レスポンス HEADERS が到着しても
    /// 接続エラーにならず破棄される
    ///
    /// `handle_headers` の GOAWAY 送信後チェックは `closed_streams` を考慮するため、
    /// リセット済みストリームへの遅延 HEADERS を新規ストリームとして
    /// 接続エラーに昇格しない。
    /// RFC 9113 Section 5.1: RST_STREAM 送信で closed 状態になったストリームへの
    /// 遅延フレームは最小処理して破棄する。
    #[test]
    fn test_reset_stream_delayed_headers_after_goaway() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        client
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");
        client
            .send_goaway(ErrorCode::NoError, vec![])
            .expect("send_goaway should succeed");

        // リセット済みストリームへの遅延レスポンス HEADERS を受信しても接続は維持される
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_response_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        let headers_bytes = encode_frame(&Frame::Headers(headers));
        client.feed(&headers_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        // 破棄されたヘッダーは HeadersReceived イベントにならない
        assert_no_event(
            &mut client,
            |e| matches!(e, Event::HeadersReceived { .. }),
            "リセット済みストリームへの遅延 HEADERS は破棄されるべき",
        );
    }

    /// リセット時に送信バッファに残データがあるストリームが、接続レベル WINDOW_UPDATE 受信で
    /// DATA を送信されない
    ///
    /// リセット済みストリームは `streams` から即時削除されるため、
    /// `flush_all_stream_data` の対象にならない。修正前は Closed 状態のまま残り、
    /// 接続レベル WINDOW_UPDATE 受信時に RST_STREAM 送信後に DATA を送信する
    /// (RFC 9113 Section 5.4.2 の「RST_STREAM はそのストリームに送信できる最後のフレーム」
    /// 違反) バグがあった。
    #[test]
    fn test_reset_stream_send_buffer_not_flushed() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");

        // 接続レベルの送信ウィンドウ (65535) を枯渇させ、ストリーム 5 の送信バッファに
        // 残データを残す (ストリーム 5 の送信ウィンドウは 61000 残っている)
        client
            .send_data(client_stream_id(1), vec![0u8; 1000], false)
            .expect("send_data should succeed");
        client
            .send_data(client_stream_id(3), vec![0u8; 60000], false)
            .expect("send_data should succeed");
        client
            .send_data(client_stream_id(5), vec![0u8; 4535], false)
            .expect("send_data should succeed");
        client
            .send_data(client_stream_id(5), vec![0u8; 10000], false)
            .expect("send_data should succeed");

        // 送信バッファに残データがある状態でリセットする
        client
            .reset_stream(client_stream_id(5), ErrorCode::Cancel)
            .expect("reset_stream should succeed");

        // リセットまでの出力 (HEADERS / DATA / RST_STREAM) を消費してから、
        // 接続レベル WINDOW_UPDATE 受信後の出力のみを検査する
        let _ = client.poll_output();

        // 接続レベル WINDOW_UPDATE を受信しても、リセット済みストリームの
        // 残データは送信されない
        let wu_frame = Frame::WindowUpdate(WindowUpdateFrame::for_connection(
            WindowIncrement::from_static(5000),
        ));
        let wu_bytes = encode_frame(&wu_frame);
        client.feed(&wu_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        // WINDOW_UPDATE は処理されている (接続レベル WINDOW_UPDATE の受信イベントが通知される)
        assert!(
            find_event(&mut client, |e| matches!(
                e,
                Event::WindowUpdateReceived {
                    stream_id: shiguredo_http2::StreamId::Connection,
                    ..
                }
            )),
            "接続レベル WINDOW_UPDATE で WindowUpdateReceived が通知されるべき"
        );

        let output = client.poll_output();
        let mut saw_data = false;
        if let Some(bytes) = output {
            let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
            decoder.feed(&bytes);
            while let Some(frame) = decoder.decode().expect("decode should succeed") {
                if matches!(frame, Frame::Data(_)) {
                    saw_data = true;
                }
            }
        }
        assert!(
            !saw_data,
            "リセット済みストリームの残データは送信されてはならない"
        );
    }

    /// リセット済みストリームへの遅延 WINDOW_UPDATE (正常値) のウィンドウ更新効果は破棄され、
    /// 接続が維持される
    ///
    /// リセット済みストリームは `streams` に存在しないため、ストリーム向け WINDOW_UPDATE の
    /// ウィンドウ更新効果は破棄されるが、`Event::WindowUpdateReceived` は通知される
    /// (受信 RST_STREAM で削除されたストリームと同じ既存の挙動。
    /// RFC 9113 Section 6.9: closed 状態のストリームへの WINDOW_UPDATE 受信はエラーと
    /// してはならない (MUST NOT))。
    #[test]
    fn test_reset_stream_delayed_window_update_received() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        client
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");
        // 明示リセット由来の Event::StreamReset を消費する
        while client.poll_event().is_some() {}

        // リセット済みストリームへのストリーム向け WINDOW_UPDATE を受信しても接続は維持される
        let wu_frame = Frame::WindowUpdate(WindowUpdateFrame::for_stream(
            NonZeroStreamId::from_static(1),
            WindowIncrement::from_static(100),
        ));
        let wu_bytes = encode_frame(&wu_frame);
        client.feed(&wu_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        // WindowUpdateReceived は通知される
        assert!(
            find_event(&mut client, |e| matches!(
                e,
                Event::WindowUpdateReceived {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    ..
                }
            )),
            "リセット済みストリームへの WINDOW_UPDATE で WindowUpdateReceived が通知されるべき"
        );
    }

    /// リセット済みストリームへの遅延 RST_STREAM は無視され、接続が維持される
    ///
    /// `handle_rst_stream` は `check_not_idle_stream` を通過した後、`streams` に存在しない
    /// ストリームへの RST_STREAM を無視する (RFC 9113 Section 5.1 の closed 状態への
    /// 遅延フレームの扱い)。
    #[test]
    fn test_reset_stream_delayed_rst_stream_ignored() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        client
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");
        // 明示リセット由来の Event::StreamReset を消費する
        while client.poll_event().is_some() {}

        // リセット済みストリームへの遅延 RST_STREAM を受信しても接続は維持される
        let rst_frame = Frame::RstStream(RstStreamFrame::new(
            NonZeroStreamId::from_static(1),
            ErrorCode::Cancel.as_u32(),
        ));
        let rst_bytes = encode_frame(&rst_frame);
        client.feed(&rst_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        // 遅延 RST_STREAM 由来の Event::StreamReset は通知されない
        assert_no_event(
            &mut client,
            |e| matches!(e, Event::StreamReset { .. }),
            "リセット済みストリームへの遅延 RST_STREAM で Event::StreamReset が push されてはならない",
        );
    }

    /// GOAWAY 送信後に未開設ストリームの HEADERS が届くと接続エラーになる
    ///
    /// `handle_headers` の GOAWAY 送信後チェックは、リセット済みストリーム (closed_streams に
    /// 登録済み) を新規ストリームとして扱わない。逆に、一度も開かれていないストリームへの
    /// HEADERS は従来どおり PROTOCOL_ERROR の接続エラーになる (RFC 9113 Section 5.1.1 の
    /// unexpected stream identifier の扱いを準用した実装判断)。
    #[test]
    fn test_new_stream_headers_after_goaway_is_error() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        client
            .send_goaway(ErrorCode::NoError, vec![])
            .expect("send_goaway should succeed");

        // GOAWAY 送信後に未開設ストリーム (stream 3) の HEADERS を受信
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(3),
            encode_valid_request_headers(),
        )
        .with_end_headers(true);
        let headers_bytes = encode_frame(&Frame::Headers(headers));
        client.feed(&headers_bytes).expect("feed should succeed");

        let result = client.process();
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.is_connection_error());
            assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }
}
