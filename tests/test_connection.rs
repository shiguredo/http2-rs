//! HTTP/2 接続の単体テスト
//!
//! PBT では到達しない意図的なエラーパスとデフォルト値境界のテスト。

use shiguredo_http2::{
    Connection, ErrorCode, Event, HeaderField, HpackEncoder, LastStreamId, Limits, NonZeroStreamId,
    WindowIncrement, WindowSize,
    frame::{
        ContinuationFrame, DataFrame, Frame, FrameDecoder, FrameEncoder, FrameFlags, FrameHeader,
        GoawayFrame, HeadersFrame, PingFrame, RstStreamFrame, SettingsFrame, WindowUpdateFrame,
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

    /// Content-Length: 5 のリクエストヘッダーを受信して既存イベントを消費する
    ///
    /// ヘッダー受信由来のイベント (HeadersReceived 等) を消費してから返すため、
    /// 呼び出し後は DATA 送信によるイベントだけを検証できる。
    fn receive_content_length_request(server: &mut Connection) {
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_request_headers_with_content_length("5"),
        )
        .with_end_headers(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}
    }

    /// 有効なレスポンスヘッダー (:status 200) を HPACK エンコードする
    fn encode_valid_response_headers() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![HeaderField::new(":status", "200").expect("valid header field")];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// Content-Length 付きのリクエストヘッダーを HPACK エンコードする
    fn encode_request_headers_with_content_length(content_length: &str) -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let mut headers = request_headers();
        headers
            .push(HeaderField::new("content-length", content_length).expect("valid header field"));
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// :status 204 のレスポンスヘッダーを HPACK エンコードする
    ///
    /// 204 はコンテンツを持たないレスポンスであり (RFC 9110 Section 15.3.5)、
    /// クライアントロールで受信すると no-content 違反の検出対象になる。
    fn encode_no_content_response_headers() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![HeaderField::new(":status", "204").expect("valid header field")];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// :status 100 の情報レスポンスヘッダーを HPACK エンコードする
    ///
    /// 1xx 情報レスポンスに END_STREAM を付けたものは malformed であり
    /// (RFC 9113 Section 8.1 の規則、Section 8.1.1 の定義)、`process_headers` が
    /// 状態遷移 (状態機械 `recv_headers`) を完了させた後にストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) を送信し、ストリームを `streams` から削除する。
    fn encode_informational_response_headers() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![HeaderField::new(":status", "100").expect("valid header field")];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// HEAD リクエストヘッダーを生成する
    ///
    /// HEAD リクエストへのレスポンスはコンテンツを持たない (RFC 9110 Section 9.3.2)。
    /// `request_headers()` の要素 0 が `:method` であることを前提とする。
    fn head_request_headers() -> Vec<HeaderField> {
        let mut headers = request_headers();
        headers[0] = HeaderField::new(":method", "HEAD").expect("valid header field");
        headers
    }

    /// Content-Length 付きレスポンスヘッダーを HPACK エンコードする
    ///
    /// コンテンツを持たないレスポンスは非ゼロ Content-Length を持つことが合法である
    /// (RFC 9113 Section 8.1.1 の「MAY have a non-zero content-length header field」)。
    fn encode_response_headers_with_content_length(status: &str, content_length: &str) -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![
            HeaderField::new(":status", status).expect("valid header field"),
            HeaderField::new("content-length", content_length).expect("valid header field"),
        ];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// クライアント開始ストリーム ID を生成する
    fn client_stream_id(id: u32) -> shiguredo_http2::StreamId {
        shiguredo_http2::StreamId::Client(shiguredo_http2::ClientStreamId::from_static(id))
    }

    /// イベントキューから条件に一致するイベントを探す
    ///
    /// 注意: 検査済みイベントは消費されるため、「存在検証」と「非存在検証」を
    /// 同じキューに対して順次行うと、後者の検証が恒真になる。
    /// 複数の条件を同時に検証する場合は [`collect_events`] を使うこと。
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

    /// イベントキュー全体を取り出す
    ///
    /// 複数の条件 (存在と非存在) を同じイベント集合に対して同時に検証するために使う。
    fn collect_events(conn: &mut Connection) -> Vec<Event> {
        let mut events = Vec::new();
        while let Some(event) = conn.poll_event() {
            events.push(event);
        }
        events
    }

    /// イベントキューに条件に一致するイベントが無いことを検証する
    fn assert_no_event<F>(conn: &mut Connection, pred: F, message: &str)
    where
        F: FnMut(&Event) -> bool,
    {
        assert!(!find_event(conn, pred), "{}", message);
    }

    /// 指定したストリーム ID の RST_STREAM フレームが出力に含まれることを検証する
    ///
    /// 出力バッファ全体を消費するため、検査対象の出力がすべて揃った後に呼ぶこと。
    /// エラーコードまで検証したい場合は [`assert_rst_stream_sent_with_code`] を使うこと。
    fn assert_rst_stream_sent(conn: &mut Connection, stream_id: u32, message: &str) {
        assert_rst_stream_sent_with_code(conn, stream_id, None, message);
    }

    /// 指定したストリーム ID の RST_STREAM フレームが出力に含まれることを検証する
    ///
    /// `error_code` に `Some` を指定した場合は、ワイヤ上のエラーコードの一致も検証する。
    /// 出力バッファ全体を消費するため、検査対象の出力がすべて揃った後に呼ぶこと。
    fn assert_rst_stream_sent_with_code(
        conn: &mut Connection,
        stream_id: u32,
        error_code: Option<ErrorCode>,
        message: &str,
    ) {
        let output = conn
            .poll_output()
            .expect("RST_STREAM フレームが出力されるべき");
        let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
        // クライアントの出力には接続プリフェイスが含まれるため、除去してからデコードする
        let frame_bytes = if output.starts_with(shiguredo_http2::CONNECTION_PREFACE) {
            &output[shiguredo_http2::CONNECTION_PREFACE_LEN..]
        } else {
            &output[..]
        };
        decoder.feed(frame_bytes);
        let mut found_rst = false;
        while let Some(frame) = decoder.decode().expect("decode should succeed") {
            if let Frame::RstStream(rst) = frame
                && rst.stream_id.as_u32() == stream_id
                && error_code.is_none_or(|code| rst.error_code == code.as_u32())
            {
                found_rst = true;
            }
        }
        assert!(found_rst, "{}", message);
    }

    /// ストリームエラーによる内部リセットの結果をまとめて検証する
    ///
    /// 指定したエラーコードの `Event::StreamReset` の通知 (接続ウィンドウ消費量込み)・
    /// RST_STREAM フレームの出力 (エラーコード込み)・違反した DATA が
    /// `Event::DataReceived` にならないことを検証する。出力バッファ全体を消費するため、
    /// 検査対象の出力がすべて揃った後に呼ぶこと。
    ///
    /// イベントの「存在」と「非存在」は同じイベント集合に対して検証する
    /// (`find_event` は検査済みイベントを消費するため、順次検証すると非存在検証が恒真になる)。
    ///
    /// 破棄経路 (DataDiscarded) と違反処理経路 (StreamReset) は排他であり、
    /// 同じ DATA フレームで両方が通知されることはない (二重通知はアプリの二重補充を招く)
    /// ため、DataDiscarded が生成されていないことも併せて検証する。
    fn assert_internal_reset(
        conn: &mut Connection,
        stream_id: u32,
        error_code: ErrorCode,
        expected_consumed: usize,
        label: &str,
    ) {
        let events = collect_events(conn);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::StreamReset {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    error_code: code,
                    connection_window_consumed,
                    ..
                } if id.as_u32() == stream_id
                    && *code == error_code
                    && *connection_window_consumed == expected_consumed
            )),
            "{label}で Event::StreamReset が通知されるべき (接続ウィンドウ消費量 {expected_consumed})"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::DataReceived { .. })),
            "{label}の DATA は DataReceived イベントになってはならない"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::DataDiscarded { .. })),
            "{label}の DATA で DataDiscarded が生成されてはならない (StreamReset と排他)"
        );
        assert_rst_stream_sent_with_code(
            conn,
            stream_id,
            Some(error_code),
            &format!("{label}で RST_STREAM フレームが出力されるべき"),
        );
    }

    /// HEADERS 経路のストリームエラーによるリセット結果をまとめて検証する
    ///
    /// `process_headers` のストリームエラー経路 (状態遷移前のヘッダー検証エラー・
    /// `recv_headers` 状態遷移エラー・状態遷移後の malformed 検出 (1xx + END_STREAM /
    /// Content-Length 不一致)) で、指定したエラーコードの `Event::StreamReset`
    /// (接続ウィンドウ消費量 0) の通知・RST_STREAM フレームの出力 (エラーコード込み)・
    /// `Event::HeadersReceived` / `Event::TrailersReceived` / `Event::StreamClosed` の
    /// 非生成を一括検証する。出力バッファ全体を消費するため、
    /// 検査対象の出力がすべて揃った後に呼ぶこと。
    ///
    /// 接続ウィンドウ消費量は HEADERS フレームがフロー制御の対象外であるため
    /// 常に 0 である (RFC 9113 Section 5.2.1: フロー制御の対象は DATA のみ)。
    ///
    /// イベントの「存在」と「非存在」は同じイベント集合に対して検証する
    /// (`find_event` は検査済みイベントを消費するため、順次検証すると非存在検証が恒真になる)。
    fn assert_headers_reset_events(
        conn: &mut Connection,
        stream_id: u32,
        error_code: ErrorCode,
        label: &str,
    ) {
        let events = collect_events(conn);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::StreamReset {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    error_code: code,
                    connection_window_consumed: 0,
                } if id.as_u32() == stream_id && *code == error_code
            )),
            "{label}で Event::StreamReset ({error_code:?}) が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::HeadersReceived { .. })),
            "{label}で Event::HeadersReceived が生成されてはならない"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::TrailersReceived { .. })),
            "{label}で Event::TrailersReceived が生成されてはならない"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamClosed { .. })),
            "{label}で Event::StreamClosed が生成されてはならない"
        );
        assert_rst_stream_sent_with_code(
            conn,
            stream_id,
            Some(error_code),
            &format!("{label}で RST_STREAM フレームが出力されるべき"),
        );
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

        // 受信ウィンドウ 4096 を超過する DATA (4097 バイト) で内部リセットされる
        // (StreamReset 通知 (接続ウィンドウ消費量 4097 込み)・RST_STREAM 出力・
        // DataReceived 非生成を一括検証する)
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 4097],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_internal_reset(
            &mut server,
            1,
            ErrorCode::FlowControlError,
            4097,
            "フロー制御違反",
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
                    ..
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
                    ..
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
        // 公開 API の reset_stream は接続ウィンドウ消費量を知らないため 0 が通知される
        assert!(
            find_event(&mut server, |e| matches!(
                e,
                Event::StreamReset {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    error_code: ErrorCode::Cancel,
                    connection_window_consumed: 0,
                    ..
                }
            )),
            "Event::StreamReset が接続ウィンドウ消費量 0 で通知されるべき"
        );

        // RST_STREAM フレームが出力されている
        assert_rst_stream_sent(&mut server, 1, "RST_STREAM フレームが出力されるべき");

        // 内部リセットは Event::StreamClosed を通知しない
        assert_no_event(
            &mut server,
            |e| matches!(e, Event::StreamClosed { .. }),
            "明示リセットで Event::StreamClosed が push されてはならない",
        );
    }

    /// ピアからの RST_STREAM 受信で `Event::StreamReset` が通知される
    ///
    /// `handle_rst_stream` は受信した RST_STREAM をそのまま `Event::StreamReset` として
    /// 通知する。RST_STREAM 受信自体は DATA の破棄ではないため、
    /// `connection_window_consumed` は 0 で通知される。
    #[test]
    fn test_peer_rst_stream_pushes_event() {
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

        // ピア RST_STREAM がそのまま Event::StreamReset (CANCEL) として通知される
        assert!(
            find_event(&mut server, |e| matches!(
                e,
                Event::StreamReset {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    error_code: ErrorCode::Cancel,
                    connection_window_consumed: 0,
                    ..
                }
            )),
            "ピア RST_STREAM で Event::StreamReset が接続ウィンドウ消費量 0 で通知されるべき"
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
    /// idle 以外のストリームへの送信は既存挙動を維持する (今回の修正は idle のみを拒否する)。
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

        // RST_STREAM フレームは送信される (既存挙動の維持)
        assert_rst_stream_sent(
            &mut server,
            1,
            "クローズ済みストリームへの RST_STREAM は送信されるべき",
        );
    }

    /// idle ストリームへの明示 `reset_stream` はエラーを返し、RST_STREAM を送信しない
    ///
    /// RFC 9113 Section 6.4: idle ストリームへの RST_STREAM 送信は MUST NOT で禁止されており、
    /// 受信したピアは PROTOCOL_ERROR の接続エラーにする。一度も開かれていないストリームへの
    /// 明示リセットは RST_STREAM を送信せずエラーを返す。
    #[test]
    fn test_reset_stream_on_idle_stream_is_error() {
        // idle ストリーム (一度も開かれていない)
        let mut idle_server = setup_server();
        while idle_server.poll_event().is_some() {}
        // セットアップ時の出力 (SETTINGS とその ACK) を消費しておく
        let _ = idle_server.poll_output();

        // idle ストリームへの明示リセットはエラーを返し、RST_STREAM を送信しない
        let result = idle_server.reset_stream(client_stream_id(1), ErrorCode::Cancel);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.is_stream_error());
            assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }

        // RST_STREAM 未送信のため出力は増えず、Event::StreamReset も push されない
        assert!(
            idle_server.poll_output().is_none(),
            "idle ストリームへの明示リセットで RST_STREAM が送信されてはならない"
        );
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

    /// 偶数ストリーム ID への明示 `reset_stream` はエラーを返す
    ///
    /// ストリーム ID の奇偶は RFC 9113 Section 5.1.1 で定められており (クライアントは奇数、
    /// サーバーは偶数)、本実装はサーバープッシュ非サポートのためサーバー開始ストリームが
    /// 存在しない。したがって偶数ストリーム ID は常に idle であり、RST_STREAM は送信されない。
    #[test]
    fn test_reset_stream_on_even_stream_id_is_error() {
        let mut server = setup_server();
        // last_recv_stream_id を 3 まで進めておき、偶数 ID の判定だけがエラーを生むことを
        // 保証する (last_recv_stream_id 超過の判定では idle にならない)
        open_stream_on_server(&mut server, 1);
        open_stream_on_server(&mut server, 3);
        // セットアップ時の出力を消費しておく
        let _ = server.poll_output();

        let result = server.reset_stream(
            shiguredo_http2::StreamId::Server(shiguredo_http2::ServerStreamId::from_static(2)),
            ErrorCode::Cancel,
        );
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.is_stream_error());
            assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }

        // RST_STREAM 未送信のため出力は増えず、Event::StreamReset も push されない
        assert!(
            server.poll_output().is_none(),
            "偶数ストリーム ID への明示リセットで RST_STREAM が送信されてはならない"
        );
        assert_no_event(
            &mut server,
            |e| matches!(e, Event::StreamReset { .. }),
            "偶数ストリーム ID への明示リセットで Event::StreamReset が push されてはならない",
        );
    }

    /// リセット済みストリームへの再リセットは既存挙動 (RST_STREAM 送信のみ) を維持する
    ///
    /// クライアントが送信開始したストリーム (last_recv_stream_id 超過) は closed_streams に
    /// 登録済みのため、`is_idle_stream` の closed_streams 考慮により idle と判定されず、
    /// 2 回目の明示リセットでも RST_STREAM が送信される。
    #[test]
    fn test_reset_stream_twice_sends_rst() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        client
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");

        // リセット済みストリームへの 2 回目の明示リセットは既存挙動を維持する
        client
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");

        // 2 回目の明示リセットの RST_STREAM フレームが出力されている
        assert_rst_stream_sent(
            &mut client,
            1,
            "リセット済みストリームへの再リセットで RST_STREAM は送信されるべき",
        );
    }

    /// `StreamId::Connection` への明示 `reset_stream` は接続エラー (PROTOCOL_ERROR) を返す
    ///
    /// RFC 9113 Section 6.4: RST_STREAM は非ゼロストリーム ID に関連付けなければならない。
    /// idle 検査を追加しても、ストリーム ID 0 の既存のエラー種別は変わらない。
    #[test]
    fn test_reset_stream_on_connection_id_is_error() {
        let mut server = setup_server();

        let result = server.reset_stream(shiguredo_http2::StreamId::Connection, ErrorCode::Cancel);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.is_connection_error());
            assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// ピアがストリーム ID を飛ばしたことで暗黙的にクローズ済みになったストリームへの
    /// 明示 `reset_stream` は既存挙動 (RST_STREAM 送信のみ) を維持する
    ///
    /// RFC 9113 Section 5.1.1: より大きい ID のストリームが開かれると、スキップされた
    /// ストリームは暗黙的に closed に遷移する。closed 状態は idle ではないため、
    /// 明示リセットはエラーにならず RST_STREAM が送信される。
    /// なお、closed 状態への RST_STREAM 送信は RFC 9113 Section 5.1 の
    /// 「PRIORITY 以外を送信してはならない (MUST NOT)」に厳密には抵触しうるが、
    /// 既存挙動を維持する。
    #[test]
    fn test_reset_stream_implicitly_closed_stream_sends_rst() {
        let mut server = setup_server();
        // ストリーム 1 と 5 を受信 (ストリーム 3 はスキップ → 暗黙的にクローズ済み)
        open_stream_on_server(&mut server, 1);
        open_stream_on_server(&mut server, 5);

        // スキップされたストリーム 3 への明示リセットは既存挙動 (RST_STREAM 送信のみ) を維持する
        server
            .reset_stream(client_stream_id(3), ErrorCode::Cancel)
            .expect("reset_stream should succeed");

        // streams に存在しないため Event::StreamReset は push されない
        assert_no_event(
            &mut server,
            |e| matches!(e, Event::StreamReset { .. }),
            "暗黙的クローズ済みストリームへの明示リセットで Event::StreamReset が push されてはならない",
        );

        // RST_STREAM フレームは送信される (既存挙動の維持)
        assert_rst_stream_sent(
            &mut server,
            3,
            "暗黙的クローズ済みストリームへの RST_STREAM は送信されるべき",
        );
    }

    /// Content-Length 超過の DATA による内部リセットで `Event::StreamReset` が通知され、
    /// 接続が維持される
    ///
    /// RFC 9113 Section 8.1.1: Content-Length と DATA ペイロード長の合計が一致しない
    /// メッセージは malformed であり、PROTOCOL_ERROR のストリームエラーとして
    /// 処理しなければならない (MUST)。ストリームエラーは RST_STREAM で処理され
    /// (RFC 9113 Section 5.4.2)、接続は維持される。
    #[test]
    fn test_content_length_exceeded_pushes_stream_reset() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // Content-Length 5 を超える 6 バイトの DATA を送信する
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 6],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        // ストリームエラーが接続エラーとして伝播せず、process は成功する
        server.process().expect("process should succeed");

        assert_internal_reset(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            6,
            "Content-Length 超過",
        );
    }

    /// END_STREAM 時の Content-Length 不一致による内部リセットで `Event::StreamReset` が
    /// 通知され、接続が維持される
    ///
    /// RFC 9113 Section 8.1.1: END_STREAM で受信が完了した時点の DATA ペイロード長の合計が
    /// Content-Length と一致しないメッセージは malformed であり、PROTOCOL_ERROR の
    /// ストリームエラーとして処理しなければならない (MUST)。
    #[test]
    fn test_content_length_mismatch_on_end_stream_pushes_stream_reset() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // Content-Length 5 に対して 3 バイトのみの DATA を END_STREAM 付きで送信する
        let data_frame =
            DataFrame::new(NonZeroStreamId::from_static(1), vec![0u8; 3]).with_end_stream(true);
        let data_bytes = encode_frame(&Frame::Data(data_frame));
        server.feed(&data_bytes).expect("feed should succeed");
        // ストリームエラーが接続エラーとして伝播せず、process は成功する
        server.process().expect("process should succeed");

        assert_internal_reset(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            3,
            "END_STREAM 時の Content-Length 不一致",
        );
    }

    /// Content-Length と受信データがちょうど一致する正常系で DATA が受理される
    ///
    /// RFC 9113 Section 8.1.1: Content-Length と DATA ペイロード長の合計が一致する
    /// メッセージは正常であり、ストリームエラーにならない。
    /// 複数 DATA フレームに分割された累積一致と、END_STREAM 時一致の境界値を検証する。
    #[test]
    fn test_content_length_exact_match_accepts_data() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // Content-Length 5 ちょうどを 2 + 3 バイトの 2 つの DATA に分割して送信する
        let data1 = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 2],
        )));
        server.feed(&data1).expect("feed should succeed");
        server.process().expect("process should succeed");

        let data2 =
            DataFrame::new(NonZeroStreamId::from_static(1), vec![0u8; 3]).with_end_stream(true);
        let data2 = encode_frame(&Frame::Data(data2));
        server.feed(&data2).expect("feed should succeed");
        // 正常系のためストリームエラーにならず、process は成功する
        server.process().expect("process should succeed");

        // 2 つの DATA がそれぞれ DataReceived イベントとして通知されている
        let events = collect_events(&mut server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataReceived {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    data,
                    end_stream: false,
                } if data.len() == 2
            )),
            "1 つ目の DATA が DataReceived イベントとして通知されるべき"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataReceived {
                    stream_id: shiguredo_http2::StreamId::Client(_),
                    data,
                    end_stream: true,
                } if data.len() == 3
            )),
            "2 つ目の DATA が DataReceived イベントとして通知されるべき"
        );

        // ストリームエラーは発生しない (同じイベント集合に対して同時に検証する)
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "Content-Length 一致の正常系で Event::StreamReset が push されてはならない"
        );
    }

    /// no-content レスポンス (204) への DATA による内部リセットで `Event::StreamReset` が
    /// 通知され、接続が維持される
    ///
    /// RFC 9113 Section 8.1.1: 204/304/HEAD はコンテンツを持たない (RFC 9110
    /// Section 6.4.1)。内容を持つ DATA を受信したメッセージは malformed であり、
    /// PROTOCOL_ERROR のストリームエラーとして処理しなければならない (MUST)。
    /// `no_content` はクライアントロールのレスポンス受信時のみ設定される。
    #[test]
    fn test_no_content_violation_pushes_stream_reset() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // :status 204 のレスポンスヘッダーを受信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_no_content_response_headers(),
        )
        .with_end_headers(true);
        client
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        // ヘッダー受信由来の既存イベントを消費する
        while client.poll_event().is_some() {}

        // no-content レスポンスへの内容を持つ DATA を送信する
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![1, 2, 3],
        )));
        client.feed(&data_bytes).expect("feed should succeed");
        // ストリームエラーが接続エラーとして伝播せず、process は成功する
        client.process().expect("process should succeed");

        assert_internal_reset(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            3,
            "no-content 違反",
        );
    }

    /// no-content レスポンス (204) への空 DATA (END_STREAM 付き) が許容され、
    /// `Event::DataReceived` が通知されること
    ///
    /// ストリームは Closed に遷移し `Event::StreamClosed` も通知される
    /// (END_STREAM 付きリクエスト送信で HalfClosedLocal → 空 DATA + END_STREAM 受信で Closed)。
    ///
    /// RFC 9113 Section 6.1: ゼロ長 DATA + END_STREAM はストリーム終端の合法的な手段。
    /// RFC 9110 Section 6.4.1: no-content (204) はコンテンツの不在であり、
    /// 0 バイト DATA はコンテンツを形成しない。
    #[test]
    fn test_no_content_empty_data_with_end_stream_accepted() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // :status 204 のレスポンスヘッダーを受信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_no_content_response_headers(),
        )
        .with_end_headers(true);
        client
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        while client.poll_event().is_some() {}

        // no-content レスポンスへの空 DATA + END_STREAM は許容される
        let data_frame =
            DataFrame::new(NonZeroStreamId::from_static(1), Vec::new()).with_end_stream(true);
        let data_bytes = encode_frame(&Frame::Data(data_frame));
        client.feed(&data_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    data,
                    end_stream: true,
                } if id.as_u32() == 1 && data.is_empty()
            )),
            "no-content レスポンスへの空 DATA (END_STREAM 付き) は DataReceived として通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "no-content レスポンスへの空 DATA で StreamReset が push されてはならない"
        );
        // ゼロ長 DATA + END_STREAM はストリーム終端の合法的な手段であり (RFC 9113 Section 6.1)、
        // ストリームは Closed に遷移して StreamClosed が通知される
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::StreamClosed {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                } if id.as_u32() == 1
            )),
            "no-content レスポンスへの空 DATA + END_STREAM で StreamClosed が通知されるべき"
        );
        // イベント集合は DataReceived + StreamClosed のちょうど 2 件である
        assert_eq!(
            events.len(),
            2,
            "no-content レスポンスへの空 DATA + END_STREAM で通知されるのは DataReceived と StreamClosed のみであるべき"
        );
    }

    /// no-content レスポンス (204) への空 DATA (END_STREAM なし) が許容され、
    /// `Event::DataReceived` のみ通知されること
    #[test]
    fn test_no_content_empty_data_without_end_stream_accepted() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // :status 204 のレスポンスヘッダーを受信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_no_content_response_headers(),
        )
        .with_end_headers(true);
        client
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        while client.poll_event().is_some() {}

        // no-content レスポンスへの空 DATA (END_STREAM なし) は許容される
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            Vec::new(),
        )));
        client.feed(&data_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    data,
                    end_stream: false,
                } if id.as_u32() == 1 && data.is_empty()
            )),
            "no-content レスポンスへの空 DATA (END_STREAM なし) は DataReceived として通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "no-content レスポンスへの空 DATA で StreamReset が push されてはならない"
        );
        // イベント集合は DataReceived のみである (ストリームは Open のまま)
        assert_eq!(
            events.len(),
            1,
            "no-content レスポンスへの空 DATA (END_STREAM なし) で通知されるのは DataReceived のみであるべき"
        );
    }

    /// 非ゼロ Content-Length を持つ no-content レスポンス (HEAD + `Content-Length: N`) への
    /// 空 DATA + END_STREAM が許容され、Content-Length チェックでリセットされないこと
    ///
    /// RFC 9113 Section 8.1.1: コンテンツを持たないレスポンスは非ゼロ Content-Length を
    /// 持つことが合法 (「MAY have a non-zero content-length header field」)。
    /// HEAD レスポンスはコンテンツを持たない (RFC 9110 Section 6.4.1 / Section 9.3.2)。
    #[test]
    fn test_no_content_head_with_content_length_empty_data_accepted() {
        let mut client = setup_client();
        client
            .start_stream(head_request_headers(), false)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // HEAD リクエストへの :status 200 + Content-Length: 5 のレスポンスヘッダーを受信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_response_headers_with_content_length("200", "5"),
        )
        .with_end_headers(true);
        client
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        while client.poll_event().is_some() {}

        // 空 DATA + END_STREAM は許容され、Content-Length 不一致でリセットされない
        let data_frame =
            DataFrame::new(NonZeroStreamId::from_static(1), Vec::new()).with_end_stream(true);
        let data_bytes = encode_frame(&Frame::Data(data_frame));
        client.feed(&data_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    data,
                    end_stream: true,
                } if id.as_u32() == 1 && data.is_empty()
            )),
            "HEAD + Content-Length: N への空 DATA (END_STREAM 付き) は DataReceived として通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "HEAD + Content-Length: N への空 DATA で StreamReset が push されてはならない"
        );
        // イベント集合は DataReceived のみである
        // (リクエストが END_STREAM なしのため、空 DATA + END_STREAM 受信では
        // HalfClosedRemote に遷移し StreamClosed は通知されない)
        assert_eq!(
            events.len(),
            1,
            "HEAD + Content-Length: N への空 DATA で通知されるのは DataReceived のみであるべき"
        );
    }

    /// no-content レスポンス (204) へのパディングのみの DATA (データ長 0 + パディング) が
    /// 許容されること
    ///
    /// フレームデコード後の `frame.data` が空になるため、no-content チェックは
    /// コンテンツの有無のみを判定する (RFC 9113 Section 6.1 のフロー制御は
    /// パディングを含むペイロード全体に適用される)。
    #[test]
    fn test_no_content_padding_only_data_accepted() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // :status 204 のレスポンスヘッダーを受信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_no_content_response_headers(),
        )
        .with_end_headers(true);
        client
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        while client.poll_event().is_some() {}

        // パディングのみの DATA (データ長 0 + パディング 5) は許容される。
        // フレームデコード後の data が空のため no-content チェックを通過する。
        // なお、接続ウィンドウはペイロード全体 (1 + 5 = 6 バイト) 消費されるが、
        // アプリは data.len() (0) しか知覚できず補充できない。パディング分の
        // 接続ウィンドウ消費量はアプリに通知されず、本テストではウィンドウ消費量の
        // 検証は対象外。
        let padding_only_bytes = encode_frame(&Frame::Data(
            DataFrame::new(NonZeroStreamId::from_static(1), Vec::new()).with_padding(5),
        ));
        client
            .feed(&padding_only_bytes)
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    data,
                    end_stream: false,
                } if id.as_u32() == 1 && data.is_empty()
            )),
            "no-content レスポンスへのパディングのみ DATA は DataReceived として通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "no-content レスポンスへのパディングのみ DATA で StreamReset が push されてはならない"
        );
        // イベント集合は DataReceived のみである (ストリームは Open のまま)
        assert_eq!(
            events.len(),
            1,
            "no-content レスポンスへのパディングのみ DATA で通知されるのは DataReceived のみであるべき"
        );
    }

    /// no-content レスポンス (204) へのパディングのみの DATA (データ長 0 + パディング) +
    /// END_STREAM が許容され、ストリームが Closed に遷移して `Event::StreamClosed` が
    /// 通知されること
    ///
    /// ゼロ長 DATA + END_STREAM はストリーム終端の合法的な手段であり
    /// (RFC 9113 Section 6.1)、パディングはコンテンツを形成しない
    /// (RFC 9110 Section 6.4.1)。
    #[test]
    fn test_no_content_padding_only_data_with_end_stream_closes_stream() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // :status 204 のレスポンスヘッダーを受信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_no_content_response_headers(),
        )
        .with_end_headers(true);
        client
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        while client.poll_event().is_some() {}

        // パディングのみ DATA (データ長 0 + パディング 5) + END_STREAM は許容される
        let padding_only_bytes = encode_frame(&Frame::Data(
            DataFrame::new(NonZeroStreamId::from_static(1), Vec::new())
                .with_padding(5)
                .with_end_stream(true),
        ));
        client
            .feed(&padding_only_bytes)
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    data,
                    end_stream: true,
                } if id.as_u32() == 1 && data.is_empty()
            )),
            "no-content レスポンスへのパディングのみ DATA + END_STREAM は DataReceived として通知されるべき"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::StreamClosed {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                } if id.as_u32() == 1
            )),
            "no-content レスポンスへのパディングのみ DATA + END_STREAM で StreamClosed が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "no-content レスポンスへのパディングのみ DATA で StreamReset が push されてはならない"
        );
        // イベント集合は DataReceived + StreamClosed のちょうど 2 件である
        assert_eq!(
            events.len(),
            2,
            "no-content レスポンスへのパディングのみ DATA + END_STREAM で通知されるのは DataReceived と StreamClosed のみであるべき"
        );
    }

    /// HalfClosedRemote 状態のストリームへの DATA による内部リセットで
    /// `Event::StreamReset` が通知され、接続が維持される
    ///
    /// RFC 9113 Section 5.1: half-closed (remote) 状態のストリームへの DATA は
    /// STREAM_CLOSED のストリームエラーとして処理しなければならない (MUST)。
    #[test]
    fn test_data_on_half_closed_remote_pushes_stream_reset() {
        let mut server = setup_server();

        // END_STREAM 付きのリクエストヘッダーを受信して HalfClosedRemote にする
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        // ヘッダー受信由来の既存イベントを消費する
        while server.poll_event().is_some() {}

        // HalfClosedRemote 状態のストリームへの DATA を送信する
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![1, 2, 3],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        // ストリームエラーが接続エラーとして伝播せず、process は成功する
        server.process().expect("process should succeed");

        assert_internal_reset(
            &mut server,
            1,
            ErrorCode::StreamClosed,
            3,
            "HalfClosedRemote 状態への DATA",
        );
    }

    /// ストリームエラーでリセットされたストリームへの遅延 DATA は破棄され、接続が維持される
    ///
    /// Content-Length 超過でリセットされたストリーム (streams から削除済み) への
    /// 遅延 DATA は `handle_data` のクローズ済みチェックで破棄され、接続は維持される
    /// (RFC 9113 Section 5.1: closed 状態への遅延フレームは最小処理して破棄する)。
    /// 遅延 DATA が再度リセットを発生させないことも検証する。
    #[test]
    fn test_stream_error_reset_delayed_data_discarded() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // Content-Length 5 を超える DATA でリセットを発生させる
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 6],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");
        // リセット由来の Event::StreamReset と出力を消費する
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // リセット済みストリームへの遅延 DATA を受信しても接続は維持される
        let delayed_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![9, 9, 9],
        )));
        server.feed(&delayed_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        // 破棄されたデータは DataReceived イベントにならず、リセットも再発しない
        // (同じイベント集合に対して同時に検証する)
        let events = collect_events(&mut server);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::DataReceived { .. })),
            "リセット済みストリームへの遅延 DATA は破棄されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "リセット済みストリームへの遅延 DATA で Event::StreamReset が再発してはならない"
        );

        // 遅延 DATA で RST_STREAM が再出力されないことも検証する
        assert!(
            server.poll_output().is_none(),
            "リセット済みストリームへの遅延 DATA で RST_STREAM が再出力されてはならない"
        );
    }

    /// 攻撃シナリオの経路遷移を検証する
    ///
    /// 1 回目の違反 DATA (Content-Length 超過) で `Event::StreamReset` の
    /// `connection_window_consumed` が通知され、その後同一ストリーム ID への
    /// 遅延 DATA で `Event::DataDiscarded` が通知される。
    ///
    /// ストリームエラーでリセットされたストリームは `streams` から削除されるため、
    /// 遅延 DATA は「`streams` マップに存在しない場合」の `DataDiscarded` 経路に入る。
    #[test]
    fn test_stream_error_then_delayed_data_reports_discarded() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // 1 回目の違反 DATA (Content-Length 超過、6 バイト) で内部リセットされる
        // (StreamReset 通知・RST_STREAM 出力・DataReceived 非生成を一括検証する)
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 6],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");
        assert_internal_reset(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            6,
            "Content-Length 超過",
        );

        // 同一ストリーム ID への遅延 DATA (3 バイト) は破棄され、DataDiscarded が通知される
        let delayed_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![9, 9, 9],
        )));
        server.feed(&delayed_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        let events = collect_events(&mut server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 3,
                } if id.as_u32() == 1
            )),
            "遅延 DATA 破棄で DataDiscarded が接続ウィンドウ消費量つきで通知されるべき"
        );
    }

    /// 通常クローズ (END_STREAM 受信) 後にストリームが `streams` から削除され、
    /// そのストリームへの遅延 DATA で `Event::DataDiscarded` が通知される
    ///
    /// クライアントが END_STREAM 付きリクエストを送信 (HalfClosedLocal) した後に
    /// サーバーから END_STREAM 付きレスポンス HEADERS を受信するとストリームは
    /// Closed 状態になり、`recv_headers` のクローズ処理で即座に `streams` から
    /// 削除される。この削除済みストリームへの遅延 DATA は「`streams` マップに
    /// 存在しない場合」の `DataDiscarded` 経路で破棄され、接続ウィンドウ消費量が
    /// 通知される。
    #[test]
    fn test_data_discarded_after_normal_close() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // END_STREAM 付きレスポンス HEADERS を受信して Closed にする
        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_response_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        while client.poll_event().is_some() {}

        // Closed 状態のストリームへの遅延 DATA (4 バイト) は破棄される
        let delayed_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![1, 2, 3, 4],
        )));
        client.feed(&delayed_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 4,
                } if id.as_u32() == 1
            )),
            "クローズ済みストリームへの遅延 DATA で DataDiscarded が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::DataReceived { .. })),
            "クローズ済みストリームへの遅延 DATA は破棄されるべき"
        );
    }

    /// END_STREAM 付き情報レスポンス (1xx) の malformed がストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に
    /// 変換され、接続が維持される
    ///
    /// `recv_headers` は状態遷移を完了させてから (HalfClosedLocal + END_STREAM で
    /// Closed に遷移) 情報レスポンス (1xx) の END_STREAM 違反を検出する
    /// (RFC 9113 Section 8.1 / 8.1.1: malformed)。malformed はストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) を送信し、`Event::StreamReset` を通知して
    /// `streams` から削除する (RFC 9113 Section 5.4.2)。
    /// エラー経路で削除されたストリームへの遅延 DATA は破棄され、
    /// `Event::DataDiscarded` で接続ウィンドウ消費量が通知される
    /// (`Event::StreamReset` は再発しない)。
    #[test]
    fn test_malformed_1xx_end_stream_resets_stream() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // END_STREAM 付き情報レスポンス (1xx) は malformed であり、
        // 状態遷移 (HalfClosedLocal + END_STREAM → Closed) 後に
        // ストリームエラーとして RST_STREAM (PROTOCOL_ERROR) に変換される
        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_informational_response_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        // Event::StreamReset (PROTOCOL_ERROR, 接続ウィンドウ消費量 0) が通知され、
        // Event::HeadersReceived / TrailersReceived / StreamClosed は生成されない
        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "1xx + END_STREAM malformed",
        );

        // streams から削除済みのストリームへの遅延 DATA (4 バイト) は破棄され、
        // Event::DataDiscarded が通知される (Event::StreamReset は再発しない)
        let delayed_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![1, 2, 3, 4],
        )));
        client.feed(&delayed_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 4,
                } if id.as_u32() == 1
            )),
            "削除済みストリームへの遅延 DATA で DataDiscarded が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "削除済みストリームへの遅延 DATA で StreamReset が再発してはならない"
        );
    }

    /// END_STREAM 付きリクエストの Content-Length 不一致 (malformed) が
    /// ストリームエラーとして RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` +
    /// `streams` 削除に変換され、接続が維持される (サーバーロール)
    ///
    /// `recv_headers` は状態遷移を完了させてから (Idle + END_STREAM で
    /// half-closed (remote) に遷移) Content-Length 不一致を検出する
    /// (RFC 9113 Section 8.1.1: malformed)。malformed はストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) を送信し、`Event::StreamReset` を通知して
    /// `streams` から削除する (RFC 9113 Section 5.4.2)。
    #[test]
    fn test_malformed_content_length_end_stream_resets_stream_server() {
        let mut server = setup_server();

        // Content-Length: 5 のリクエストを END_STREAM 付きで受信すると
        // END_STREAM 時の Content-Length 不一致 (malformed) になる
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_request_headers_with_content_length("5"),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        // Event::StreamReset (PROTOCOL_ERROR, 接続ウィンドウ消費量 0) が通知され、
        // Event::HeadersReceived / TrailersReceived / StreamClosed は生成されない
        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "Content-Length 不一致 (サーバー)",
        );

        // ストリームが streams から削除済みであることを区別可能に検証する
        // (この経路は half-closed (remote) に遷移するため、もしマップに残っていたら
        // 遅延 DATA は recv_data の状態遷移エラーで RST_STREAM (STREAM_CLOSED) が
        // 再発するはずであり、削除済みなら DataDiscarded になる)
        let delayed_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![1, 2, 3],
        )));
        server.feed(&delayed_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        let events = collect_events(&mut server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 3,
                } if id.as_u32() == 1
            )),
            "リセット済みストリームへの遅延 DATA で DataDiscarded が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "リセット済みストリームへの遅延 DATA で StreamReset が再発してはならない"
        );
    }

    /// END_STREAM 付きレスポンスの Content-Length 不一致 (malformed) が
    /// ストリームエラーとして RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` +
    /// `streams` 削除に変換され、接続が維持される (クライアントロール)
    ///
    /// `recv_headers` は状態遷移を完了させてから (HalfClosedLocal + END_STREAM で
    /// Closed に遷移) Content-Length 不一致を検出する
    /// (RFC 9113 Section 8.1.1: malformed)。malformed はストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) を送信し、`Event::StreamReset` を通知して
    /// `streams` から削除する (RFC 9113 Section 5.4.2)。
    #[test]
    fn test_malformed_content_length_end_stream_resets_stream_client() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // :status 200 + Content-Length: 5 のレスポンスを END_STREAM 付きで受信すると
        // END_STREAM 時の Content-Length 不一致 (malformed) になる
        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_response_headers_with_content_length("200", "5"),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        // Event::StreamReset (PROTOCOL_ERROR, 接続ウィンドウ消費量 0) が通知され、
        // Event::HeadersReceived / TrailersReceived / StreamClosed は生成されない
        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "Content-Length 不一致 (クライアント)",
        );
    }

    /// リクエストボディ送信中 (Open) のクライアントが END_STREAM 付きレスポンスの
    /// Content-Length 不一致を受信すると half-closed (remote) からリセットされ、
    /// ストリームが streams から削除される
    ///
    /// `start_stream(..., false)` で送信したリクエストは END_STREAM なしのため、
    /// 受信時点の状態は Open であり、recv_headers (END_STREAM) により
    /// half-closed (remote) に遷移してから Content-Length 不一致
    /// (RFC 9113 Section 8.1.1: malformed) が検出される。この経路は
    /// half-closed (remote) への RST_STREAM 送信であり、RFC 9113 Section 5.1 の
    /// closed 状態への送信制限には抵触しない。
    #[test]
    fn test_malformed_content_length_open_state_resets_stream() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // :status 200 + Content-Length: 5 のレスポンスを END_STREAM 付きで受信すると
        // Content-Length 不一致 (malformed) になり、ストリームエラーとして処理される
        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_response_headers_with_content_length("200", "5"),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "Content-Length 不一致 (Open 状態)",
        );

        // ストリームが streams から削除済みであることを区別可能に検証する
        // (Open 状態からは half-closed (remote) に遷移するため、マップに残っていたら
        // 遅延 DATA は recv_data の状態遷移エラーで RST_STREAM (STREAM_CLOSED) が再発する
        // はずであり、削除済みなら DataDiscarded になる)
        let delayed_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![1, 2, 3],
        )));
        client.feed(&delayed_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 3,
                } if id.as_u32() == 1
            )),
            "リセット済みストリームへの遅延 DATA で DataDiscarded が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "リセット済みストリームへの遅延 DATA で StreamReset が再発してはならない"
        );
    }

    /// コンテンツを持たないレスポンス (204) は非ゼロ Content-Length を持つことが
    /// 合法であり、END_STREAM 付きでもリセットされず `Event::HeadersReceived` になる
    ///
    /// RFC 9113 Section 8.1.1: 「A response that is defined to have no content ... MAY
    /// have a non-zero content-length header field」。Content-Length 不一致チェックの
    /// skip_check (204/304/HEAD) の正例であり、HEADERS 経路のリセット変換が
    /// 誤って適用されないことを検証する。
    #[test]
    fn test_no_content_content_length_end_stream_headers_accepted() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        // リクエスト送信由来のイベントと出力を消費する
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // :status 204 + Content-Length: 5 のレスポンスを END_STREAM 付きで受信しても
        // リセットされず、Event::HeadersReceived が通知される
        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_response_headers_with_content_length("204", "5"),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        let events = collect_events(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::HeadersReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    end_stream: true,
                    ..
                } if id.as_u32() == 1
            )),
            "no-content レスポンス + 非ゼロ Content-Length + END_STREAM で HeadersReceived が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "no-content レスポンスの Content-Length 不一致チェックはスキップされるべき"
        );
    }

    /// END_STREAM 付きリクエストの Content-Length: 0 は合法であり、
    /// リセットされず `Event::HeadersReceived` になる
    ///
    /// RFC 9113 Section 8.1.1: Content-Length と DATA ペイロード長の不一致が
    /// malformed の条件であり、END_STREAM 時の Content-Length チェックは
    /// 非ゼロのみを対象とする (`content_length.is_some_and(|len| len != 0)` の
    /// 合法側境界)。
    #[test]
    fn test_content_length_zero_end_stream_headers_accepted() {
        let mut server = setup_server();

        // Content-Length: 0 のリクエストを END_STREAM 付きで受信しても
        // リセットされず、Event::HeadersReceived が通知される
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_request_headers_with_content_length("0"),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        let events = collect_events(&mut server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::HeadersReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    end_stream: true,
                    ..
                } if id.as_u32() == 1
            )),
            "Content-Length: 0 + END_STREAM で HeadersReceived が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "Content-Length: 0 + END_STREAM で StreamReset が生成されてはならない"
        );
    }

    /// ストリームエラーで RST_STREAM を送信したストリームが GOAWAY の last-stream-id に
    /// 含まれる
    ///
    /// RFC 9113 Section 6.8: last-stream-id は「sender が何らかの action を取った
    /// かもしれない最高番号のストリーム ID」であり、RST_STREAM 送信もこの action に
    /// 該当する。ヘッダー処理が成功したストリームと同様に、
    /// ストリームエラーでリセットしたストリームも `last_successful_stream_id` の
    /// 更新対象に含める。
    #[test]
    fn test_reset_stream_included_in_goaway_last_stream_id() {
        let mut server = setup_server();

        // Content-Length: 5 のリクエストを END_STREAM 付きで受信して
        // Content-Length 不一致 (malformed) を発生させ、RST_STREAM (PROTOCOL_ERROR) を
        // 送信させる (ストリーム ID 1)
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_request_headers_with_content_length("5"),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        // ストリームエラー由来のイベントと出力を消費する
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // GOAWAY を送信し、last-stream-id にストリーム ID 1 が含まれることを検証する
        server
            .send_goaway(ErrorCode::NoError, Vec::new())
            .expect("send_goaway should succeed");
        let output = server
            .poll_output()
            .expect("GOAWAY フレームが出力されるべき");
        let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
        decoder.feed(&output);
        let mut found_goaway = false;
        while let Some(frame) = decoder.decode().expect("decode should succeed") {
            if let Frame::Goaway(goaway) = frame {
                assert_eq!(
                    goaway.last_stream_id.get(),
                    1,
                    "GOAWAY の last-stream-id に RST_STREAM 送信済みストリームが含まれるべき"
                );
                found_goaway = true;
            }
        }
        assert!(found_goaway, "GOAWAY フレームが出力されるべき");
    }

    /// CONTINUATION に分割されたヘッダーブロックでも Content-Length 不一致の
    /// ストリームエラーが RST_STREAM (PROTOCOL_ERROR) 送信 + streams 削除に変換され、
    /// 接続が維持される
    ///
    /// `handle_continuation` も `process_headers` を経由するため、単一 HEADERS フレームと
    /// 同じリセット経路を通る。分割送信の後始末 (header_block_fragment のクリア等) と
    /// リセットの相互作用も併せて検証する。
    #[test]
    fn test_malformed_content_length_end_stream_resets_stream_continuation() {
        let mut server = setup_server();

        // Content-Length: 5 のリクエストを HEADERS + CONTINUATION に分割して送信する
        let encoded = encode_request_headers_with_content_length("5");
        let split = encoded.len() / 2;
        let headers = HeadersFrame::new(NonZeroStreamId::from_static(1), encoded[..split].to_vec())
            .with_end_headers(false)
            .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        let continuation = create_continuation(
            NonZeroStreamId::from_static(1),
            encoded[split..].to_vec(),
            true,
        );
        server
            .feed(&encode_frame(&Frame::Continuation(continuation)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        // 単一 HEADERS フレームと同じく、ストリームエラーとして処理される
        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "Content-Length 不一致 (CONTINUATION 分割)",
        );

        // CONTINUATION 経由でもリセット済みストリームへの遅延 DATA は破棄される
        let delayed_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![1, 2, 3],
        )));
        server.feed(&delayed_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        let events = collect_events(&mut server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 3,
                } if id.as_u32() == 1
            )),
            "リセット済みストリームへの遅延 DATA で DataDiscarded が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "リセット済みストリームへの遅延 DATA で StreamReset が再発してはならない"
        );
    }

    /// 空 DATA の破棄では `Event::DataDiscarded` が生成されないこと
    #[test]
    fn test_empty_data_discarded_no_event() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // Content-Length 超過でリセットする
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 6],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // クローズ済みストリームへの空 DATA は破棄されるが DataDiscarded は通知されない
        let empty_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            Vec::new(),
        )));
        server.feed(&empty_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_no_event(
            &mut server,
            |e| matches!(e, Event::DataDiscarded { .. }),
            "空 DATA の破棄で DataDiscarded が生成されてはならない",
        );
    }

    /// パディング付き遅延 DATA の破棄で接続ウィンドウ消費量に
    /// ペイロード全体 (Pad Length フィールド + データ + パディング) が計上されること
    ///
    /// RFC 9113 Section 6.1: フロー制御は DATA フレームのペイロード全体に適用され、
    /// Pad Length フィールドとパディングを含む。破棄経路でも同じ計上量が
    /// `Event::DataDiscarded` の `connection_window_consumed` で通知される。
    #[test]
    fn test_padded_data_discarded_counts_padding() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // Content-Length 超過でリセットする
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 6],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // パディング付き遅延 DATA (データ 2 バイト + パディング 5 バイト) は破棄される。
        // 接続ウィンドウ消費量は Pad Length フィールド (1) + データ (2) + パディング (5) = 8
        let padded_bytes = encode_frame(&Frame::Data(
            DataFrame::new(NonZeroStreamId::from_static(1), vec![1, 2]).with_padding(5),
        ));
        server.feed(&padded_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        let events = collect_events(&mut server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 8,
                } if id.as_u32() == 1
            )),
            "パディング付き DATA の破棄で Pad Length 込みの接続ウィンドウ消費量が通知されるべき"
        );
    }

    /// パディング付き違反 DATA の内部リセットで `Event::StreamReset` の
    /// `connection_window_consumed` にペイロード全体 (Pad Length フィールド +
    /// データ + パディング) が計上されること
    ///
    /// RFC 9113 Section 6.1: フロー制御は DATA フレームのペイロード全体に適用される。
    /// ストリームエラー経路の `Event::StreamReset` も `DataDiscarded` と同じ
    /// `flow_control_size` で計上する。
    #[test]
    fn test_padded_violation_data_counts_padding_in_stream_reset() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // Content-Length 5 を超過するパディング付き違反 DATA。
        // データ 6 バイト (Content-Length 超過、RFC 9113 Section 8.1.1) + パディング 5 バイト。
        // ペイロード全体は Pad Length フィールド (1) + データ (6) + パディング (5) = 12
        let padded_bytes = encode_frame(&Frame::Data(
            DataFrame::new(NonZeroStreamId::from_static(1), vec![0u8; 6]).with_padding(5),
        ));
        server.feed(&padded_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_internal_reset(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            12,
            "パディング付き Content-Length 超過",
        );
    }

    /// データ 0 + パディングのみの遅延 DATA の破棄で `Event::DataDiscarded` が通知されること
    ///
    /// 破棄判定は「data が空か」ではなく「`flow_control_size` が 0 か」で行う
    /// (RFC 9113 Section 6.1: フロー制御は Pad Length フィールドとパディングも含む)。
    /// パディングのみ DATA は `flow_control_size = 1 + 0 + 5 = 6` で通知される。
    #[test]
    fn test_padding_only_data_discarded_notifies_consumed() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // Content-Length 超過でリセットする
        let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(1),
            vec![0u8; 6],
        )));
        server.feed(&data_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // パディングのみ (データ 0 + パディング 5) の遅延 DATA は破棄され、
        // 接続ウィンドウ消費量 6 (Pad Length フィールド 1 + パディング 5) が通知される
        let padding_only_bytes = encode_frame(&Frame::Data(
            DataFrame::new(NonZeroStreamId::from_static(1), Vec::new()).with_padding(5),
        ));
        server
            .feed(&padding_only_bytes)
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        let events = collect_events(&mut server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 6,
                } if id.as_u32() == 1
            )),
            "パディングのみ DATA の破棄で接続ウィンドウ消費量が通知されるべき"
        );
    }

    /// 接続ウィンドウを補充しない場合、破棄 DATA の消費で接続ウィンドウが枯渇し
    /// `FLOW_CONTROL_ERROR` の接続エラーになること (動機の再現)
    #[test]
    fn test_connection_window_exhaustion_without_replenishment() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // 接続ウィンドウ (デフォルト 65535) を枯渇させるため、大きな DATA で違反を繰り返す。
        // 1 回目は Content-Length 超過で StreamReset、2 回目以降は遅延 DATA 破棄。
        // どちらの経路も接続ウィンドウを消費する (RFC 9113 Section 6.9 の MUST)。
        // 16384 バイト × 4 回 = 65536 > 65535 のため 4 回目で枯渇し、
        // ループ回数 (5 回) は 4 回目までの累積が必ず上限を超える根拠に依存している。
        let payload_size = 16_384usize;
        let mut exhausted = false;
        let mut iteration = 0;
        for _ in 0..5 {
            iteration += 1;
            let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
                NonZeroStreamId::from_static(1),
                vec![0u8; payload_size],
            )));
            server.feed(&data_bytes).expect("feed should succeed");
            if let Err(e) = server.process() {
                // 接続ウィンドウ枯渇で FLOW_CONTROL_ERROR の接続エラー
                assert!(e.is_connection_error(), "枯渇時は接続エラーになるべき");
                assert_eq!(e.error_code(), Some(ErrorCode::FlowControlError));
                exhausted = true;
                break;
            }
            while server.poll_event().is_some() {}
            let _ = server.poll_output();
        }
        assert!(
            exhausted,
            "接続ウィンドウを補充しないと FLOW_CONTROL_ERROR で遮断されるべき"
        );
        assert_eq!(
            iteration, 4,
            "16384 バイト × 4 回で接続ウィンドウ (65535) を超過するため 4 回目で枯渇するべき"
        );
    }

    /// 接続ウィンドウを補充した場合、破棄 DATA の消費を補充して
    /// 正当なストリームの DATA 受信が継続できること (修正の効果)
    #[test]
    fn test_connection_window_replenishment_keeps_connection() {
        let mut server = setup_server();
        receive_content_length_request(&mut server);

        // 違反 DATA と補充を繰り返しても接続が維持されること
        let payload_size = 16_384usize;
        for _ in 0..5 {
            let data_bytes = encode_frame(&Frame::Data(DataFrame::new(
                NonZeroStreamId::from_static(1),
                vec![0u8; payload_size],
            )));
            server.feed(&data_bytes).expect("feed should succeed");
            server.process().expect("process should succeed");

            // 通知された接続ウィンドウ消費量を補充する
            let consumed = collect_events(&mut server)
                .iter()
                .filter_map(|e| match e {
                    Event::StreamReset {
                        connection_window_consumed,
                        ..
                    }
                    | Event::DataDiscarded {
                        connection_window_consumed,
                        ..
                    } => Some(*connection_window_consumed),
                    _ => None,
                })
                .sum::<usize>();
            assert!(
                consumed > 0,
                "破棄 DATA の接続ウィンドウ消費量が通知されるべき"
            );
            let _ = server.poll_output();

            server
                .send_window_update(shiguredo_http2::StreamId::Connection, consumed as u32)
                .expect("send_window_update should succeed");
            while server.poll_event().is_some() {}
            let _ = server.poll_output();
        }

        // 接続が維持され、正当なストリーム (ID 3) の DATA 受信が継続できる
        open_stream_on_server(&mut server, 3);
        let ok_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(3),
            vec![1, 2, 3],
        )));
        server.feed(&ok_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        assert!(
            find_event(&mut server, |e| matches!(
                e,
                Event::DataReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    ..
                } if id.as_u32() == 3
            )),
            "補充後は正当なストリーム (ID 3) の DATA が受信できるべき"
        );
    }

    /// 疑似ヘッダーを含まないヘッダーブロックを HPACK エンコードする
    ///
    /// 初回 HEADERS の疑似ヘッダー欠如 (RFC 9113 Section 8.1 / 8.3.1: malformed) の
    /// テストで使用する。
    fn encode_headers_without_pseudo() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers =
            vec![HeaderField::new("content-type", "text/html").expect("valid header field")];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// 有効なトレーラーヘッダーを HPACK エンコードする
    fn encode_valid_trailer_headers() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![HeaderField::new("x-trailer", "1").expect("valid header field")];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// トレーラーで禁止されたヘッダー (te) を含むトレーラーを HPACK エンコードする
    ///
    /// RFC 9113 Section 8.2.2: TE ヘッダーの例外はリクエストのみであり、
    /// トレーラーでは禁止される (validate_forbidden_header_for_response)。
    fn encode_trailer_headers_with_forbidden() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![HeaderField::new("te", "trailers").expect("valid header field")];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// :method を欠いたリクエストヘッダーを HPACK エンコードする
    ///
    /// RFC 9113 Section 8.3.1: 必須疑似ヘッダー (:method) の欠如は malformed
    /// (validate_request_headers が MissingPseudoHeader を返す)。
    fn encode_request_headers_missing_method() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let mut headers = request_headers();
        headers.retain(|h| h.name() != b":method");
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// :status 101 のレスポンスヘッダーを HPACK エンコードする
    ///
    /// RFC 9113 Section 8.6: HTTP/2 は 101 (Switching Protocols) をサポートしない
    /// (validate_response_headers が Status101NotSupported を返す)。
    fn encode_response_headers_status_101() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![HeaderField::new(":status", "101").expect("valid header field")];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// Extended CONNECT リクエスト (CONNECT + :protocol) を HPACK エンコードする
    ///
    /// RFC 8441: Extended CONNECT は :protocol 疑似ヘッダーを含む。
    /// ENABLE_CONNECT_PROTOCOL 未設定のサーバーが検出するネゴシエーション違反の
    /// テストで使用する。
    fn encode_extended_connect_request_headers() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![
            HeaderField::new(":method", "CONNECT").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":protocol", "webtransport").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
        ];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// Content-Length: abc (パース不能) のリクエストヘッダーを HPACK エンコードする
    ///
    /// RFC 9110 Section 8.6: content-length は 1 桁以上の数字であり、パース不能な値は
    /// malformed (extract_content_length がエラーを返す)。
    fn encode_request_headers_with_invalid_content_length() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let mut headers = request_headers();
        headers.push(HeaderField::new("content-length", "abc").expect("valid header field"));
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// Content-Length: abc (パース不能) のレスポンスヘッダーを HPACK エンコードする
    ///
    /// RFC 9110 Section 8.6 の根拠は
    /// [`encode_request_headers_with_invalid_content_length`] を参照。
    fn encode_response_headers_with_invalid_content_length() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![
            HeaderField::new(":status", "200").expect("valid header field"),
            HeaderField::new("content-length", "abc").expect("valid header field"),
        ];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// リセット済みストリームへの遅延 DATA が破棄され、`Event::DataDiscarded` が通知され、
    /// `Event::StreamReset` が再発しないことを検証する
    ///
    /// 遅延 DATA が接続エラーにならず破棄され、接続が維持されることの回帰確認。
    /// ストリームが open / half-closed などの非 Closed 状態のままマップに残存する
    /// 旧挙動 (孤立ストリーム) では、遅延 DATA が `recv_data` の状態遷移エラーで
    /// RST_STREAM (STREAM_CLOSED) を再発させるため、`Event::StreamReset` の非存在で
    /// その回帰を検出できる。
    ///
    /// なお `handle_data` の破棄判定 (`src/connection.rs` の `is_stream_closed` /
    /// `!streams.contains_key`) は Closed 状態でマップに残存するストリームにも
    /// `Event::DataDiscarded` を生成するため、本検証は「マップから削除されたこと」の
    /// 厳密な証明にはならない。削除の裏付けは `assert_headers_reset_events` の
    /// `Event::StreamReset` 存在検証 (リセット時に `streams` から削除される) が担う。
    fn assert_delayed_data_discarded(conn: &mut Connection, stream_id: u32) {
        let delayed_bytes = encode_frame(&Frame::Data(DataFrame::new(
            NonZeroStreamId::from_static(stream_id),
            vec![1, 2, 3],
        )));
        conn.feed(&delayed_bytes).expect("feed should succeed");
        conn.process().expect("process should succeed");

        let events = collect_events(conn);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DataDiscarded {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    connection_window_consumed: 3,
                } if id.as_u32() == stream_id
            )),
            "リセット済みストリーム ({stream_id}) への遅延 DATA で DataDiscarded が通知されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "リセット済みストリーム ({stream_id}) への遅延 DATA で StreamReset が再発してはならない"
        );
    }

    /// 疑似ヘッダーを欠いた初回リクエスト HEADERS がストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 生成・削除に
    /// 変換され、接続が維持される (サーバーロール)
    ///
    /// RFC 9113 Section 8.1 / 8.3.1: 初回 HEADERS の疑似ヘッダー欠如は malformed であり、
    /// Section 8.1.1 に従いストリームエラーとして処理する。ストリーム未作成のため
    /// 生成してからリセットする。既存挙動 (接続終了) と異なり、ストリームエラーが
    /// `process()` から呼び出し側へ伝播せず接続が維持される。
    #[test]
    fn test_initial_headers_without_pseudo_resets_stream() {
        let mut server = setup_server();

        // 疑似ヘッダーなしの初回 HEADERS を受信する (ストリーム未作成)
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_headers_without_pseudo(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "初回 HEADERS の疑似ヘッダー欠如",
        );

        // ストリームが生成されてから削除済みであることを区別可能に検証する:
        // 遅延 DATA は DataDiscarded で破棄され、StreamReset が再発しない
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// 新規ストリームの検証エラーで生成・リセットしたストリームが GOAWAY の
    /// last-stream-id に含まれる
    ///
    /// 検証エラー経路が `Err` 返却から `Ok` 返却に変換されたことで、
    /// `handle_headers` の `last_successful_stream_id` 更新が新規ストリーム
    /// (ストリーム未作成 → `reset_headers_validation_error` で生成・リセット) の
    /// 経路でも適用されることを、GOAWAY の last-stream-id で固定する。
    /// [`test_reset_stream_included_in_goaway_last_stream_id`] が既存ストリームの
    /// 状態遷移後経路を検証するのに対し、本テストは新規ストリーム生成経路を検証する。
    /// RST_STREAM 送信も RFC 9113 Section 6.8 の last-stream-id 更新対象。
    #[test]
    fn test_reset_new_stream_included_in_goaway_last_stream_id() {
        let mut server = setup_server();

        // 疑似ヘッダーなしの初回 HEADERS で新規ストリームを生成・リセットさせる
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_headers_without_pseudo(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        // ストリームエラー由来のイベントと出力を消費する
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // GOAWAY を送信し、last-stream-id にストリーム ID 1 が含まれることを検証する
        server
            .send_goaway(ErrorCode::NoError, Vec::new())
            .expect("send_goaway should succeed");
        let output = server
            .poll_output()
            .expect("GOAWAY フレームが出力されるべき");
        let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
        decoder.feed(&output);
        let mut found_goaway = false;
        while let Some(frame) = decoder.decode().expect("decode should succeed") {
            if let Frame::Goaway(goaway) = frame {
                assert_eq!(
                    goaway.last_stream_id.get(),
                    1,
                    "GOAWAY の last-stream-id に新規ストリーム生成・リセット済みストリームが含まれるべき"
                );
                found_goaway = true;
            }
        }
        assert!(found_goaway, "GOAWAY フレームが出力されるべき");
    }

    /// 新規ストリームの検証エラーを CONTINUATION 分割で受信した場合も、
    /// ストリームが生成されてからリセットされ、接続が維持される
    ///
    /// `handle_continuation` も `process_headers` を経由するため、単一 HEADERS フレームと
    /// 同じリセット経路を通る。CONTINUATION 分割の後始末 (header_block_fragment の
    /// クリア等) と、新規ストリーム生成 → リセットの相互作用も併せて検証する。
    #[test]
    fn test_new_stream_validation_error_via_continuation_resets_stream() {
        let mut server = setup_server();

        // 疑似ヘッダーなしのヘッダーブロックを HEADERS + CONTINUATION に分割して送信する
        let encoded = encode_headers_without_pseudo();
        let split = encoded.len() / 2;
        let headers = HeadersFrame::new(NonZeroStreamId::from_static(1), encoded[..split].to_vec())
            .with_end_headers(false)
            .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        let continuation = create_continuation(
            NonZeroStreamId::from_static(1),
            encoded[split..].to_vec(),
            true,
        );
        server
            .feed(&encode_frame(&Frame::Continuation(continuation)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        // 単一 HEADERS フレームと同じく、ストリームが生成されてからリセットされる
        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "新規ストリームの検証エラー (CONTINUATION 分割)",
        );

        // CONTINUATION 経由でもリセット済みストリームへの遅延 DATA は破棄される
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// 疑似ヘッダーを欠いた初回レスポンス HEADERS がストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (クライアントロール)
    ///
    /// クライアントは `start_stream` でストリームを生成済みのため、
    /// 既存ストリームのままリセットされる。
    #[test]
    fn test_initial_response_without_pseudo_resets_stream() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // 疑似ヘッダーなしの初回レスポンスを受信する (ストリーム生成済み)
        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_headers_without_pseudo(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "初回レスポンスの疑似ヘッダー欠如",
        );
    }

    /// 初回ヘッダー受信後の疑似ヘッダーを含む HEADERS がストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される
    ///
    /// RFC 9113 Section 8.1: 疑似ヘッダーは初回ヘッダーにのみ含められる。
    #[test]
    fn test_pseudo_headers_in_non_initial_headers_resets_stream() {
        let mut server = setup_server();
        open_stream_on_server(&mut server, 1);
        // 初回 HEADERS 由来のイベントを消費する
        while server.poll_event().is_some() {}

        // 2 回目の HEADERS に疑似ヘッダーを含める
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "非初回 HEADERS の疑似ヘッダー",
        );
    }

    /// トレーラー検証エラー (トレーラーで禁止された te ヘッダー) がストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される
    ///
    /// RFC 9113 Section 8.2.2: TE ヘッダーはトレーラーでは禁止される。
    #[test]
    fn test_trailer_validation_error_resets_stream() {
        let mut server = setup_server();
        open_stream_on_server(&mut server, 1);
        // 初回 HEADERS 由来のイベントを消費する
        while server.poll_event().is_some() {}

        let trailers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_trailer_headers_with_forbidden(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(trailers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "トレーラー検証エラー",
        );
    }

    /// END_STREAM なしのトレーラーがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される
    ///
    /// RFC 9113 Section 8.1: トレーラーは END_STREAM 付きで送信しなければならない。
    #[test]
    fn test_trailer_without_end_stream_resets_stream() {
        let mut server = setup_server();
        open_stream_on_server(&mut server, 1);
        // 初回 HEADERS 由来のイベントを消費する
        while server.poll_event().is_some() {}

        let trailers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_trailer_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(false);
        server
            .feed(&encode_frame(&Frame::Headers(trailers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "END_STREAM なしトレーラー",
        );
    }

    /// :method を欠いたリクエストがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 生成・削除に
    /// 変換され、接続が維持される
    ///
    /// RFC 9113 Section 8.3.1: 必須疑似ヘッダー (:method) の欠如は malformed。
    /// validate_request_headers はストリーム生成前に実行されるため、
    /// 新規ストリームとして生成してからリセットする。
    #[test]
    fn test_request_headers_missing_method_resets_stream() {
        let mut server = setup_server();

        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_request_headers_missing_method(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(&mut server, 1, ErrorCode::ProtocolError, ":method 欠如");

        // ストリームが生成されてから削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// :status 101 のレスポンスがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される
    ///
    /// RFC 9113 Section 8.6: HTTP/2 は 101 (Switching Protocols) をサポートしない。
    #[test]
    fn test_response_headers_status_101_resets_stream() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_response_headers_status_101(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(&mut client, 1, ErrorCode::ProtocolError, ":status 101");
    }

    /// ENABLE_CONNECT_PROTOCOL 未設定のサーバーが受信した :protocol 付きリクエストが
    /// ストリームエラーとして RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` +
    /// `streams` 生成・削除に変換され、接続が維持される
    ///
    /// RFC 8441 Section 3: Extended CONNECT は SETTINGS_ENABLE_CONNECT_PROTOCOL=1 を送信済みの
    /// 場合のみ許可される。ネゴシエーション違反はプロトコルエラーであり、
    /// ストリームエラーとして処理する。
    #[test]
    fn test_protocol_without_enable_connect_protocol_resets_stream() {
        let mut server = setup_server();

        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_extended_connect_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            ":protocol ネゴシエーション違反",
        );

        // ストリームが生成されてから削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// パース不能な Content-Length のリクエストがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (サーバーロール)
    ///
    /// RFC 9110 Section 8.6: content-length は 1 桁以上の数字。パース不能な値は
    /// malformed (RFC 9113 Section 8.1.1)。
    #[test]
    fn test_invalid_content_length_resets_stream_server() {
        let mut server = setup_server();

        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_request_headers_with_invalid_content_length(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "Content-Length パースエラー (サーバー)",
        );

        // ストリームが生成されてから削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// パース不能な Content-Length のレスポンスがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (クライアントロール)
    ///
    /// RFC 9110 Section 8.6: content-length は 1 桁以上の数字。パース不能な値は
    /// malformed (RFC 9113 Section 8.1.1)。
    #[test]
    fn test_invalid_content_length_resets_stream_client() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_response_headers_with_invalid_content_length(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "Content-Length パースエラー (クライアント)",
        );
    }

    /// 符号付き Content-Length のリクエストがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (サーバーロール)
    ///
    /// RFC 9110 Section 8.6: Content-Length の ABNF は 1*DIGIT であり、`+0` のような
    /// 符号付き数字は ABNF 違反である。u64::from_str は先頭の '+' を受理するため、
    /// パース前の ASCII 数字検証で拒否する。値が 0 のため END_STREAM 時の
    /// Content-Length 不一致チェック (非ゼロ値の検出) では弾かれず、
    /// ASCII 数字検証が唯一の拒否経路であることが検証できる。
    #[test]
    fn test_signed_content_length_resets_stream_server() {
        let mut server = setup_server();

        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_request_headers_with_content_length("+0"),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "符号付き Content-Length (サーバー)",
        );

        // ストリームが生成されてから削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// 符号付き Content-Length のレスポンスがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (クライアントロール)
    ///
    /// RFC 9110 Section 8.6: Content-Length の ABNF は 1*DIGIT であり、`+0` のような
    /// 符号付き数字は ABNF 違反である。u64::from_str は先頭の '+' を受理するため、
    /// パース前の ASCII 数字検証で拒否する。値が 0 のため END_STREAM 時の
    /// Content-Length 不一致チェック (非ゼロ値の検出) では弾かれず、
    /// ASCII 数字検証が唯一の拒否経路であることが検証できる。
    #[test]
    fn test_signed_content_length_resets_stream_client() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), true)
            .expect("start_stream should succeed");
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_response_headers_with_content_length("200", "+0"),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "符号付き Content-Length (クライアント)",
        );

        // ストリームが削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut client, 1);
    }

    /// half-closed (remote) 状態のストリームへの追加 HEADERS が状態遷移エラーとして
    /// RST_STREAM (STREAM_CLOSED) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (サーバーロール)
    ///
    /// RFC 9113 Section 5.1: half-closed (remote) 状態のストリームへの
    /// WINDOW_UPDATE / PRIORITY / RST_STREAM 以外のフレームは STREAM_CLOSED の
    /// ストリームエラーで応答しなければならない (MUST)。
    #[test]
    fn test_headers_on_half_closed_remote_resets_stream() {
        let mut server = setup_server();

        // END_STREAM 付きリクエストで half-closed (remote) に遷移させる
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}

        // half-closed (remote) への追加 HEADERS (トレーラー) は状態遷移エラー
        let trailers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_trailer_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(trailers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::StreamClosed,
            "half-closed (remote) への HEADERS",
        );

        // ストリームが削除済みであることを遅延 DATA で区別可能に検証する
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// half-closed (remote) 状態のストリームへの追加 HEADERS が状態遷移エラーとして
    /// RST_STREAM (STREAM_CLOSED) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (クライアントロール)
    ///
    /// 仕様根拠は [`test_headers_on_half_closed_remote_resets_stream`] を参照。
    #[test]
    fn test_headers_on_half_closed_remote_resets_stream_client() {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        // END_STREAM 付きレスポンスで half-closed (remote) に遷移させる
        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_response_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        while client.poll_event().is_some() {}

        // half-closed (remote) への追加 HEADERS (トレーラー) は状態遷移エラー
        let trailers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_trailer_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(trailers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::StreamClosed,
            "half-closed (remote) への HEADERS (クライアント)",
        );
    }

    /// クライアント接続を初期化し、有効なレスポンスを END_STREAM なしで受信済みの状態にする
    ///
    /// ストリームは Open のまま残り `initial_headers_received` が立つため、
    /// 後続 HEADERS (非初回疑似ヘッダー・トレーラー等) の検証エラーを発生させる土台になる。
    fn setup_client_with_response() -> Connection {
        let mut client = setup_client();
        client
            .start_stream(request_headers(), false)
            .expect("start_stream should succeed");
        while client.poll_event().is_some() {}
        let _ = client.poll_output();

        let response = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_response_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(false);
        client
            .feed(&encode_frame(&Frame::Headers(response)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");
        while client.poll_event().is_some() {}
        client
    }

    /// 初回ヘッダー受信後の疑似ヘッダーを含む HEADERS がストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (クライアントロール)
    ///
    /// RFC 9113 Section 8.1: 疑似ヘッダーは初回ヘッダーにのみ含められる。
    #[test]
    fn test_pseudo_headers_in_non_initial_headers_resets_stream_client() {
        let mut client = setup_client_with_response();

        // 2 回目の HEADERS に疑似ヘッダーを含める
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "非初回 HEADERS の疑似ヘッダー (クライアント)",
        );

        // ストリームが削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut client, 1);
    }

    /// トレーラー検証エラー (トレーラーで禁止された te ヘッダー) がストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (クライアントロール)
    ///
    /// RFC 9113 Section 8.2.2: TE ヘッダーはトレーラーでは禁止される。
    #[test]
    fn test_trailer_validation_error_resets_stream_client() {
        let mut client = setup_client_with_response();

        let trailers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_trailer_headers_with_forbidden(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        client
            .feed(&encode_frame(&Frame::Headers(trailers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "トレーラー検証エラー (クライアント)",
        );

        // ストリームが削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut client, 1);
    }

    /// END_STREAM なしのトレーラーがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (クライアントロール)
    ///
    /// RFC 9113 Section 8.1: トレーラーは END_STREAM 付きで送信しなければならない。
    #[test]
    fn test_trailer_without_end_stream_resets_stream_client() {
        let mut client = setup_client_with_response();

        let trailers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_trailer_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(false);
        client
            .feed(&encode_frame(&Frame::Headers(trailers)))
            .expect("feed should succeed");
        client.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut client,
            1,
            ErrorCode::ProtocolError,
            "END_STREAM なしトレーラー (クライアント)",
        );

        // ストリームが削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut client, 1);
    }

    /// 同時ストリーム数上限超過の新規 HEADERS がストリームエラーとして
    /// RST_STREAM (REFUSED_STREAM) 送信 + `Event::StreamReset` + `streams` 生成・削除に
    /// 変換され、接続が維持される
    ///
    /// RFC 9113 Section 5.1.2: 受信した HEADERS が広告した同時ストリーム数上限を
    /// 超える場合、PROTOCOL_ERROR または REFUSED_STREAM のストリームエラーで応答
    /// することを MUST と定める。本実装は REFUSED_STREAM を選択する (Section 8.7
    /// の自動再試行を許可し、送信側 `start_stream` の同時上限超過と同じエラー
    /// コードで一貫させる)。ストリームエラーは RST_STREAM で処理して接続を維持
    /// する (Section 5.4.2)。
    ///
    /// 上限超過はデコード前に検出されるが、field block は破棄する場合でも伸長
    /// しなければならない (RFC 9113 Section 4.3 の MUST) ため、デコード完了後に
    /// ストリームを生成してからリセットする。
    #[test]
    fn test_concurrent_stream_limit_exceeded_resets_stream() {
        let limits = Limits::builder()
            .max_concurrent_streams(Some(1))
            .build()
            .expect("should succeed");
        let mut server = setup_server_with_limits(limits);
        open_stream_on_server(&mut server, 1);
        // ストリーム 1 の HEADERS 由来のイベントを消費する
        while server.poll_event().is_some() {}

        // 上限 (1) を超える新規 HEADERS (ストリーム 3) を受信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(3),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            3,
            ErrorCode::RefusedStream,
            "同時ストリーム数上限超過",
        );

        // ストリームが生成されてから削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut server, 3);
    }

    /// 同時ストリーム数上限超過の新規 HEADERS を CONTINUATION 分割で受信した場合も、
    /// CONTINUATION を吸収してからストリームが生成・リセットされ、接続が維持される
    ///
    /// 上限超過はデコード前に検出されるが、RFC 9113 Section 4.3 は破棄する場合でも
    /// field block の再組み立てと伸長を要求し、伸長しない場合は COMPRESSION_ERROR
    /// の接続エラーで終了しなければならない (MUST) ため、CONTINUATION の吸収と
    /// デコードを完了してからリセットする。リセット分岐は `process_headers` を
    /// スキップするため、`handle_continuation` の `last_successful_stream_id` 更新部
    /// を通らない。RST_STREAM 送信は RFC 9113 Section 6.8 の last-stream-id 更新
    /// 対象であるため、リセット分岐側の更新が必要であり、GOAWAY の last-stream-id
    /// に反映されることを検証する。
    #[test]
    fn test_concurrent_stream_limit_exceeded_via_continuation_resets_stream() {
        let limits = Limits::builder()
            .max_concurrent_streams(Some(1))
            .build()
            .expect("should succeed");
        let mut server = setup_server_with_limits(limits);
        open_stream_on_server(&mut server, 1);
        // ストリーム 1 の HEADERS 由来のイベントを消費する
        while server.poll_event().is_some() {}

        // 上限超過のヘッダーブロックを HEADERS + CONTINUATION に分割して送信する
        let encoded = encode_valid_request_headers();
        let split = encoded.len() / 2;
        let headers = HeadersFrame::new(NonZeroStreamId::from_static(3), encoded[..split].to_vec())
            .with_end_headers(false)
            .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        let continuation = create_continuation(
            NonZeroStreamId::from_static(3),
            encoded[split..].to_vec(),
            true,
        );
        server
            .feed(&encode_frame(&Frame::Continuation(continuation)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        // 単一 HEADERS フレームと同じく、ストリームが生成されてからリセットされる
        assert_headers_reset_events(
            &mut server,
            3,
            ErrorCode::RefusedStream,
            "同時ストリーム数上限超過 (CONTINUATION 分割)",
        );

        // CONTINUATION 経由でもリセット済みストリームへの遅延 DATA は破棄される
        assert_delayed_data_discarded(&mut server, 3);

        // GOAWAY を送信し、last-stream-id にストリーム ID 3 が含まれることを検証する
        assert_refused_stream_included_in_goaway(&mut server, 3);
    }

    /// 同時ストリーム数上限超過でリセットしたストリームが GOAWAY の last-stream-id に
    /// 含まれる (単一 HEADERS フレーム経路)
    ///
    /// リセット分岐は `process_headers` をスキップするため、`handle_headers` の
    /// `last_successful_stream_id` 更新部を通らない。RST_STREAM 送信は RFC 9113
    /// Section 6.8 の last-stream-id 更新対象であり、リセット分岐側の更新が GOAWAY
    /// の last-stream-id に反映されることを検証する。
    #[test]
    fn test_concurrent_stream_limit_exceeded_included_in_goaway_last_stream_id() {
        let limits = Limits::builder()
            .max_concurrent_streams(Some(1))
            .build()
            .expect("should succeed");
        let mut server = setup_server_with_limits(limits);
        open_stream_on_server(&mut server, 1);
        // ストリーム 1 の HEADERS 由来のイベントと出力を消費する
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // 上限超過の新規 HEADERS (ストリーム 3) を受信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(3),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        // ストリームエラー由来のイベントと出力を消費する
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        assert_refused_stream_included_in_goaway(&mut server, 3);
    }

    /// 同時ストリーム数上限超過でリセットしたストリームが GOAWAY の last-stream-id に
    /// 含まれることを検証する
    ///
    /// RFC 9113 Section 6.8: last-stream-id は「sender が何らかの action を取った
    /// かもしれない最高番号のストリーム ID」であり、RST_STREAM 送信もこの action に
    /// 該当する。出力バッファ全体を消費するため、GOAWAY 以外の出力が揃った後に
    /// 呼ぶこと。
    fn assert_refused_stream_included_in_goaway(server: &mut Connection, stream_id: u32) {
        server
            .send_goaway(ErrorCode::NoError, Vec::new())
            .expect("send_goaway should succeed");
        let output = server
            .poll_output()
            .expect("GOAWAY フレームが出力されるべき");
        let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
        decoder.feed(&output);
        let mut found_goaway = false;
        while let Some(frame) = decoder.decode().expect("decode should succeed") {
            if let Frame::Goaway(goaway) = frame {
                assert_eq!(
                    goaway.last_stream_id.get(),
                    stream_id,
                    "GOAWAY の last-stream-id に REFUSED_STREAM 送信済みストリームが含まれるべき"
                );
                found_goaway = true;
            }
        }
        assert!(found_goaway, "GOAWAY フレームが出力されるべき");
    }

    /// 同時ストリーム数上限超過時も field block がデコードされ、HPACK 状態が維持される
    ///
    /// RFC 9113 Section 4.3: field block を破棄する場合でも再組み立てして伸長する
    /// 必要があり、伸長しない場合は COMPRESSION_ERROR の接続エラーで終了しなければ
    /// ならない (MUST)。伸長しないままリセットして接続を維持すると、デコーダと
    /// ピアのエンコーダ文脈がずれ、以降の field block で COMPRESSION_ERROR の
    /// 接続エラーになる。
    ///
    /// 上限超過でリセットされたブロック (ブロック 2) を単一 HEADERS で送り、
    /// `handle_headers` のフラグ消費分岐を検証する (多フレーム版は
    /// [`test_concurrent_stream_limit_exceeded_keeps_hpack_state_via_continuation`])。
    #[test]
    fn test_concurrent_stream_limit_exceeded_keeps_hpack_state() {
        let limits = Limits::builder()
            .max_concurrent_streams(Some(1))
            .build()
            .expect("should succeed");
        let mut server = setup_server_with_limits(limits);
        assert_hpack_state_kept_after_refused(&mut server, false);
    }

    /// 同時ストリーム数上限超過の多フレーム field block (HEADERS + CONTINUATION) でも
    /// field block がデコードされ、HPACK 状態が維持される
    ///
    /// [`test_concurrent_stream_limit_exceeded_keeps_hpack_state`] の多フレーム対称
    /// テスト。上限超過のフラグ消費は `handle_continuation` の独立した分岐で行われる
    /// ため、デコード完了後の分岐が正しく実行されることを動的テーブルの同期で固定
    /// する (RFC 9113 Section 4.3 の根拠は単一フレーム版の doc 参照)。
    #[test]
    fn test_concurrent_stream_limit_exceeded_keeps_hpack_state_via_continuation() {
        let limits = Limits::builder()
            .max_concurrent_streams(Some(1))
            .build()
            .expect("should succeed");
        let mut server = setup_server_with_limits(limits);
        assert_hpack_state_kept_after_refused(&mut server, true);
    }

    /// 同時ストリーム数上限超過のリセット後も HPACK 状態が維持されることを検証する
    ///
    /// 検証方法: 1 つの `HpackEncoder` をピアに見立てて 3 つの field block を
    /// エンコードする。上限超過でリセットされたブロック (ブロック 2) がデコード
    /// されないと動的テーブルがずれ、リセット後の新規 HEADERS (ブロック 3) の
    /// 動的テーブル参照が誤った値になるか COMPRESSION_ERROR になる。正しく
    /// デコードされていれば値が一致する。
    ///
    /// この検証は、エンコーダが動的テーブルの完全一致エントリを Indexed Header
    /// Field (RFC 7541 Section 6.1) でエンコードする実装に依存する。リテラル優先
    /// の実装に変わった場合は検出力が落ちるため、実装変更時に再評価すること。
    ///
    /// `split_block2` が真の場合はブロック 2 を HEADERS + CONTINUATION に分割して
    /// 送り、`handle_continuation` のフラグ消費分岐を検証する。偽の場合は単一
    /// HEADERS で送り、`handle_headers` の分岐を検証する。
    fn assert_hpack_state_kept_after_refused(server: &mut Connection, split_block2: bool) {
        let mut encoder = HpackEncoder::new(4096);

        // ブロック 1: ストリーム 1 (上限内のため正常に処理される)
        let mut first_headers = request_headers();
        first_headers.push(HeaderField::new("x-dynamic", "v1").expect("valid header field"));
        let mut block1 = Vec::new();
        encoder.encode(&mut block1, &first_headers);
        let frame1 = HeadersFrame::new(NonZeroStreamId::from_static(1), block1)
            .with_end_headers(true)
            .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(frame1)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // ブロック 2: ストリーム 3 (上限超過 → デコード後にリセットされる)
        let mut second_headers = request_headers();
        second_headers.push(HeaderField::new("x-dynamic", "v2").expect("valid header field"));
        let mut block2 = Vec::new();
        encoder.encode(&mut block2, &second_headers);
        if split_block2 {
            // CONTINUATION 分割で送信し、多フレーム経路のフラグ消費を検証する
            let split = block2.len() / 2;
            let frame2 =
                HeadersFrame::new(NonZeroStreamId::from_static(3), block2[..split].to_vec())
                    .with_end_headers(false)
                    .with_end_stream(true);
            server
                .feed(&encode_frame(&Frame::Headers(frame2)))
                .expect("feed should succeed");
            server.process().expect("process should succeed");
            let continuation = create_continuation(
                NonZeroStreamId::from_static(3),
                block2[split..].to_vec(),
                true,
            );
            server
                .feed(&encode_frame(&Frame::Continuation(continuation)))
                .expect("feed should succeed");
            server.process().expect("process should succeed");
        } else {
            let frame2 = HeadersFrame::new(NonZeroStreamId::from_static(3), block2)
                .with_end_headers(true)
                .with_end_stream(true);
            server
                .feed(&encode_frame(&Frame::Headers(frame2)))
                .expect("feed should succeed");
            server.process().expect("process should succeed");
        }
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // ストリーム 1 をリセットして同時ストリーム数の上限を空ける
        server
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // ブロック 3: ストリーム 5 (動的テーブル参照を含み、正常に処理される)
        // ブロック 2 がデコードされていれば参照が正しく解決され、
        // されていなければ COMPRESSION_ERROR の接続エラーになる
        let mut block3 = Vec::new();
        encoder.encode(&mut block3, &second_headers);
        let frame3 = HeadersFrame::new(NonZeroStreamId::from_static(5), block3)
            .with_end_headers(true)
            .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(frame3)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        // ストリーム 5 の HEADERS が正常に処理され、x-dynamic: v2 として受信される
        let events = collect_events(server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::HeadersReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    headers,
                    ..
                } if id.as_u32() == 5
                    && headers
                        .iter()
                        .any(|h| h.name() == b"x-dynamic" && h.value() == b"v2")
            )),
            "リセット後の新規 HEADERS が正常に処理され、x-dynamic: v2 が受信されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "リセット後の新規 HEADERS で StreamReset が生成されてはならない"
        );
    }

    /// 同時ストリーム数上限が 0 のサーバーが受信した最初の新規 HEADERS が
    /// RST_STREAM (REFUSED_STREAM) でリセットされ、接続が維持される
    ///
    /// RFC 9113 Section 6.5.2: SETTINGS_MAX_CONCURRENT_STREAMS は 0 に設定でき、
    /// ゼロ値は新規ストリームの作成を防ぐ (「A value of 0 ... SHOULD NOT be
    /// treated as special by endpoints」)。上限超過の検出は既存ストリーム数に
    /// 依存しないため、最初の新規 HEADERS から REFUSED_STREAM でリセットされる
    /// (RFC 9113 Section 5.1.2)。
    #[test]
    fn test_concurrent_stream_limit_zero_resets_stream() {
        let limits = Limits::builder()
            .max_concurrent_streams(Some(0))
            .build()
            .expect("should succeed");
        let mut server = setup_server_with_limits(limits);

        // 上限 0 のため、最初の新規 HEADERS (ストリーム 1) がリセットされる
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::RefusedStream,
            "同時ストリーム数上限 0",
        );

        // ストリームが生成されてから削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// 同時ストリーム数上限超過のリセットが 2 回連続しても、フラグの往復
    /// (設定 → 消費) が正しく行われ、接続が維持される
    ///
    /// 上限超過の検出はヘッダーブロックごとに `header_concurrent_limit_exceeded` に
    /// 記録され、デコード完了時に必ず消費される。消費漏れ・残存の回帰があると、
    /// 2 回目のリセット後に正常な新規 HEADERS が誤ってリセットされるため、
    /// このテストで検出できる。
    #[test]
    fn test_concurrent_stream_limit_exceeded_twice_keeps_connection() {
        let limits = Limits::builder()
            .max_concurrent_streams(Some(1))
            .build()
            .expect("should succeed");
        let mut server = setup_server_with_limits(limits);
        open_stream_on_server(&mut server, 1);
        // ストリーム 1 の HEADERS 由来のイベントを消費する
        while server.poll_event().is_some() {}

        // 1 回目の上限超過 (ストリーム 3)
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(3),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // 2 回目の上限超過 (ストリーム 5)
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(5),
            encode_valid_request_headers(),
        )
        .with_end_headers(true)
        .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            5,
            ErrorCode::RefusedStream,
            "2 回目の同時ストリーム数上限超過",
        );

        // ストリーム 1 をリセットして上限を空け、ストリーム 7 が正常処理されることを
        // 確認する (フラグが残存していると誤ってリセットされる)
        server
            .reset_stream(client_stream_id(1), ErrorCode::Cancel)
            .expect("reset_stream should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        open_stream_on_server(&mut server, 7);
        assert!(
            find_event(&mut server, |e| matches!(
                e,
                Event::HeadersReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    ..
                } if id.as_u32() == 7
            )),
            "フラグ往復後に正常な新規 HEADERS (ストリーム 7) が処理されるべき"
        );
    }

    /// CONNECT リクエストヘッダーを HPACK エンコードする
    ///
    /// RFC 9113 Section 8.5: CONNECT リクエストは :method=CONNECT と :authority
    /// (authority-form) のみを含み、:scheme / :path は MUST で省略する。
    fn encode_connect_request_headers() -> Vec<u8> {
        let mut encoder = HpackEncoder::new(4096);
        let headers = vec![
            HeaderField::new(":method", "CONNECT").expect("valid header field"),
            HeaderField::new(":authority", "example.com:443").expect("valid header field"),
        ];
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &headers);
        buf
    }

    /// CONNECT リクエストを受信して 2xx レスポンスを送信し、CONNECT トンネルを確立する
    /// (サーバーロール)
    ///
    /// RFC 9113 Section 8.5: サーバーが通常 CONNECT に 2xx を返すと
    /// `connect_established` が設定され、以後 DATA 以外のフレームはストリームエラー
    /// になる。確立時のイベントと出力はすべて消費してから返す。
    fn establish_connect_tunnel(server: &mut Connection) {
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_connect_request_headers(),
        )
        .with_end_headers(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        server
            .send_response(
                client_stream_id(1),
                vec![HeaderField::new(":status", "200").expect("valid header field")],
                false,
            )
            .expect("send_response should succeed");
        let _ = server.poll_output();
    }

    /// CONNECT 確立済みストリームへの HEADERS がストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される (単一フレーム)
    ///
    /// RFC 9113 Section 8.5: CONNECT 確立済みストリームでは DATA または stream
    /// management フレーム (RST_STREAM / WINDOW_UPDATE / PRIORITY) 以外のフレームを
    /// ストリームエラーとして処理する MUST。HEADERS はデコード前に検出されるが、
    /// field block は破棄する場合でも伸長する必要がある (RFC 9113 Section 4.3 の
    /// MUST) ため、デコード完了後にリセットする (同時ストリーム数上限超過の
    /// 遅延リセットと同じ機構)。
    #[test]
    fn test_headers_on_established_connect_resets_stream() {
        let mut server = setup_server();
        establish_connect_tunnel(&mut server);

        // CONNECT 確立済みストリーム (stream 1) への HEADERS を送信する
        let headers = HeadersFrame::new(
            NonZeroStreamId::from_static(1),
            encode_valid_request_headers(),
        )
        .with_end_headers(true);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "CONNECT 確立済みストリームへの HEADERS",
        );

        // ストリームが streams から削除済みであることを遅延 DATA で検証する
        assert_delayed_data_discarded(&mut server, 1);
    }

    /// CONNECT 確立済みストリームへの HEADERS を CONTINUATION 分割で受信した場合も、
    /// CONTINUATION を吸収してからリセットされ、接続が維持される
    ///
    /// CONNECT 確立済みストリームへの HEADERS の検出はデコード前に行われるが、
    /// RFC 9113 Section 4.3 は破棄する場合でも field block の再組み立てと伸長を
    /// 要求する (MUST) ため、CONTINUATION の吸収とデコードを完了してからリセット
    /// する。`handle_continuation` のフラグ消費分岐を検証する。
    #[test]
    fn test_headers_on_established_connect_resets_stream_continuation() {
        let mut server = setup_server();
        establish_connect_tunnel(&mut server);

        // CONNECT 確立済みストリーム (stream 1) への HEADERS を
        // HEADERS + CONTINUATION に分割して送信する
        let encoded = encode_valid_request_headers();
        let split = encoded.len() / 2;
        let headers = HeadersFrame::new(NonZeroStreamId::from_static(1), encoded[..split].to_vec())
            .with_end_headers(false);
        server
            .feed(&encode_frame(&Frame::Headers(headers)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        let continuation = create_continuation(
            NonZeroStreamId::from_static(1),
            encoded[split..].to_vec(),
            true,
        );
        server
            .feed(&encode_frame(&Frame::Continuation(continuation)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "CONNECT 確立済みストリームへの HEADERS (CONTINUATION 分割)",
        );

        assert_delayed_data_discarded(&mut server, 1);
    }

    /// CONNECT 確立済みストリームへの未知フレームがストリームエラーとして
    /// RST_STREAM (PROTOCOL_ERROR) 送信 + `Event::StreamReset` + `streams` 削除に変換され、
    /// 接続が維持される
    ///
    /// RFC 9113 Section 8.5: 未知フレームタイプも「DATA または stream management
    /// フレーム以外」に該当し、CONNECT 確立済みストリームではストリームエラーとして
    /// 処理する MUST である。Section 4.1 の未知フレーム無視 MUST と競合するが、
    /// より特定の Section 8.5 を優先する実装判断。未知フレームはデコード済みであり
    /// HPACK 状態に影響しないため、検出時点でリセットする。
    #[test]
    fn test_unknown_frame_on_established_connect_resets_stream() {
        let mut server = setup_server();
        establish_connect_tunnel(&mut server);

        // CONNECT 確立済みストリーム (stream 1) への未知フレーム (type 0x2a) を送信する
        let unknown = Frame::Unknown {
            header: FrameHeader {
                length: 0,
                frame_type: 0x2a,
                flags: FrameFlags::empty(),
                stream_id: 1,
            },
            payload: vec![],
        };
        server
            .feed(&encode_frame(&unknown))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        assert_headers_reset_events(
            &mut server,
            1,
            ErrorCode::ProtocolError,
            "CONNECT 確立済みストリームへの未知フレーム",
        );

        assert_delayed_data_discarded(&mut server, 1);
    }

    /// CONNECT 確立済みストリームへの HEADERS のリセット後も HPACK 状態が維持される
    /// (単一フレーム)
    ///
    /// リセット分岐が `process_headers` をスキップしても、field block のデコード
    /// (伸長) は完了しているため、デコーダとピアのエンコーダ文脈がずれないことを
    /// 検証する (RFC 9113 Section 4.3)。
    #[test]
    fn test_connect_established_headers_reset_keeps_hpack_state() {
        let mut server = setup_server();
        assert_hpack_state_kept_after_connect_established(&mut server, false);
    }

    /// CONNECT 確立済みストリームへの HEADERS のリセット後も HPACK 状態が維持される
    /// (多フレーム)
    ///
    /// [`test_connect_established_headers_reset_keeps_hpack_state`] の多フレーム対称
    /// テスト。`handle_continuation` の独立した分岐でフラグを消費するため、
    /// デコード完了後の分岐が正しく実行されることを動的テーブルの同期で固定する。
    #[test]
    fn test_connect_established_headers_reset_keeps_hpack_state_via_continuation() {
        let mut server = setup_server();
        assert_hpack_state_kept_after_connect_established(&mut server, true);
    }

    /// CONNECT 確立済みストリームへの HEADERS のリセット後も HPACK 状態が維持される
    /// ことを検証する
    ///
    /// 検証方法: 1 つの `HpackEncoder` をピアに見立てて 3 つの field block を
    /// エンコードする。CONNECT 確立済みストリームへの HEADERS (ブロック 2) がデコード
    /// されないと動的テーブルがずれ、リセット後の新規 HEADERS (ブロック 3) の
    /// 動的テーブル参照が誤った値になるか COMPRESSION_ERROR になる。正しく
    /// デコードされていれば値が一致する。
    ///
    /// この検証は、エンコーダが動的テーブルの完全一致エントリを Indexed Header
    /// Field (RFC 7541 Section 6.1) でエンコードする実装に依存する。リテラル優先
    /// の実装に変わった場合は検出力が落ちるため、実装変更時に再評価すること。
    ///
    /// `split_block2` が真の場合はブロック 2 を HEADERS + CONTINUATION に分割して
    /// 送り、`handle_continuation` のフラグ消費分岐を検証する。偽の場合は単一
    /// HEADERS で送り、`handle_headers` の分岐を検証する。
    fn assert_hpack_state_kept_after_connect_established(
        server: &mut Connection,
        split_block2: bool,
    ) {
        let mut encoder = HpackEncoder::new(4096);

        // ブロック 1: stream 1 の CONNECT リクエスト (正常に処理され、2xx で確立される)
        let connect_headers = vec![
            HeaderField::new(":method", "CONNECT").expect("valid header field"),
            HeaderField::new(":authority", "example.com:443").expect("valid header field"),
        ];
        let mut block1 = Vec::new();
        encoder.encode(&mut block1, &connect_headers);
        let frame1 =
            HeadersFrame::new(NonZeroStreamId::from_static(1), block1).with_end_headers(true);
        server
            .feed(&encode_frame(&Frame::Headers(frame1)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");
        while server.poll_event().is_some() {}
        let _ = server.poll_output();
        server
            .send_response(
                client_stream_id(1),
                vec![HeaderField::new(":status", "200").expect("valid header field")],
                false,
            )
            .expect("send_response should succeed");
        let _ = server.poll_output();

        // ブロック 2: CONNECT 確立済みストリーム (stream 1) への HEADERS
        // (デコード後にリセットされる)
        let mut second_headers = request_headers();
        second_headers.push(HeaderField::new("x-dynamic", "v2").expect("valid header field"));
        let mut block2 = Vec::new();
        encoder.encode(&mut block2, &second_headers);
        if split_block2 {
            // CONTINUATION 分割で送信し、多フレーム経路のフラグ消費を検証する
            let split = block2.len() / 2;
            let frame2 =
                HeadersFrame::new(NonZeroStreamId::from_static(1), block2[..split].to_vec())
                    .with_end_headers(false);
            server
                .feed(&encode_frame(&Frame::Headers(frame2)))
                .expect("feed should succeed");
            server.process().expect("process should succeed");
            let continuation = create_continuation(
                NonZeroStreamId::from_static(1),
                block2[split..].to_vec(),
                true,
            );
            server
                .feed(&encode_frame(&Frame::Continuation(continuation)))
                .expect("feed should succeed");
            server.process().expect("process should succeed");
        } else {
            let frame2 =
                HeadersFrame::new(NonZeroStreamId::from_static(1), block2).with_end_headers(true);
            server
                .feed(&encode_frame(&Frame::Headers(frame2)))
                .expect("feed should succeed");
            server.process().expect("process should succeed");
        }
        while server.poll_event().is_some() {}
        let _ = server.poll_output();

        // ブロック 3: stream 3 の新規 HEADERS (動的テーブル参照を含み、正常に処理される)
        // ブロック 2 がデコードされていれば参照が正しく解決され、
        // されていなければ COMPRESSION_ERROR の接続エラーになる
        let mut block3 = Vec::new();
        encoder.encode(&mut block3, &second_headers);
        let frame3 = HeadersFrame::new(NonZeroStreamId::from_static(3), block3)
            .with_end_headers(true)
            .with_end_stream(true);
        server
            .feed(&encode_frame(&Frame::Headers(frame3)))
            .expect("feed should succeed");
        server.process().expect("process should succeed");

        // ストリーム 3 の HEADERS が正常に処理され、x-dynamic: v2 として受信される
        let events = collect_events(server);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::HeadersReceived {
                    stream_id: shiguredo_http2::StreamId::Client(id),
                    headers,
                    ..
                } if id.as_u32() == 3
                    && headers
                        .iter()
                        .any(|h| h.name() == b"x-dynamic" && h.value() == b"v2")
            )),
            "リセット後の新規 HEADERS が正常に処理され、x-dynamic: v2 が受信されるべき"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::StreamReset { .. })),
            "リセット後の新規 HEADERS で StreamReset が生成されてはならない"
        );
    }
}
