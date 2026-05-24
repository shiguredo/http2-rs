//! HTTP/2 接続の PBT
//!
//! RFC 9113 準拠の接続レベル検証をテストする。

use proptest::prelude::*;
use shiguredo_http2::{
    Connection, ErrorCode, HeaderField, HpackEncoder, LastStreamId, Limits, NonZeroStreamId,
    WindowIncrement,
    frame::{
        ContinuationFrame, DataFrame, Frame, FrameEncoder, GoawayFrame, HeadersFrame, PingFrame,
        RstStreamFrame, SettingsFrame, StreamId, WindowUpdateFrame,
    },
    settings::{MAX_INITIAL_WINDOW_SIZE, Setting},
};

/// 有効なストリーム ID を生成する（クライアント開始: 奇数）
fn client_stream_id() -> impl Strategy<Value = NonZeroStreamId> {
    (1u32..=100).prop_map(|n| NonZeroStreamId::new(n * 2 + 1).expect("odd value is always valid"))
}

/// フレームをバイト列にエンコードする
fn encode_frame(frame: &Frame) -> Vec<u8> {
    let mut encoder = FrameEncoder::new();
    encoder.encode(frame).unwrap();
    encoder.buffer().to_vec()
}

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

    /// idle ストリームへの RST_STREAM は PROTOCOL_ERROR
    #[test]
    fn prop_rst_stream_on_idle_is_error(stream_id in client_stream_id()) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // idle ストリームに RST_STREAM を送信
        let rst_frame =
            Frame::RstStream(RstStreamFrame::new(stream_id, ErrorCode::Cancel.as_u32()));
        let rst_bytes = encode_frame(&rst_frame);
        server.feed(&rst_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// サーバーが偶数のストリーム ID を受信した場合、PROTOCOL_ERROR
    #[test]
    fn prop_server_rejects_even_stream_id(stream_id in (1u32..=100).prop_map(|n| NonZeroStreamId::new(n * 2).expect("even non-zero is valid"))) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // 偶数ストリーム ID で HEADERS を送信
        // 有効なリクエストヘッダー (:method GET, :scheme https, :path /, :authority example.com)
        let headers = HeadersFrame::new(stream_id, encode_valid_request_headers())
            .with_end_stream(true)
            .with_end_headers(true);
        let headers_bytes = encode_frame(&Frame::Headers(headers));
        server.feed(&headers_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// クライアントがサーバーから ENABLE_PUSH=1 を受信した場合、PROTOCOL_ERROR
    #[test]
    fn prop_client_rejects_enable_push_from_server(_dummy in Just(())) {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // サーバーから ENABLE_PUSH=1 の SETTINGS を受信
        let mut settings = SettingsFrame::new();
        settings.add(Setting::EnablePush(true));
        let settings_bytes = encode_frame(&Frame::Settings(settings));
        client.feed(&settings_bytes).unwrap();

        let result = client.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// ストリーム ID が単調増加しない場合、PROTOCOL_ERROR
    #[test]
    fn prop_non_monotonic_stream_id_is_error(
        first_id_raw in (5u32..=100).prop_map(|n| n * 2 + 1),
    ) {
        let first_id = NonZeroStreamId::new(first_id_raw)
            .expect("odd non-zero is valid");
        let second_id = NonZeroStreamId::new(first_id_raw - 2)
            .expect("odd non-zero is valid"); // 単調増加していない

        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // 最初のストリーム
        // 有効なリクエストヘッダー (:method GET, :scheme https, :path /, :authority example.com)
        let headers1 = HeadersFrame::new(first_id, encode_valid_request_headers())
            .with_end_stream(true)
            .with_end_headers(true);
        let headers1_bytes = encode_frame(&Frame::Headers(headers1));
        server.feed(&headers1_bytes).unwrap();
        server.process().unwrap();

        // 小さいストリーム ID で新しいストリームを開始
        let headers2 = HeadersFrame::new(second_id, encode_valid_request_headers())
            .with_end_stream(true)
            .with_end_headers(true);
        let headers2_bytes = encode_frame(&Frame::Headers(headers2));
        server.feed(&headers2_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// 最初のフレームが SETTINGS でない場合、PROTOCOL_ERROR
    ///
    /// RFC 9113 Section 3.4: 接続プリフェイス検証
    #[test]
    fn prop_first_frame_must_be_settings(_dummy in Just(())) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS ではなく PING を最初に送信
        let ping_frame = Frame::Ping(PingFrame::new([0u8; 8]));
        let ping_bytes = encode_frame(&ping_frame);
        server.feed(&ping_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// idle ストリームへの DATA は PROTOCOL_ERROR
    ///
    /// RFC 9113 Section 5.1: idle ストリームへの DATA は PROTOCOL_ERROR
    #[test]
    fn prop_data_on_idle_stream_is_error(stream_id in client_stream_id()) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // idle ストリームに DATA を送信
        let data_frame = Frame::Data(DataFrame::new(stream_id, vec![1, 2, 3]));
        let data_bytes = encode_frame(&data_frame);
        server.feed(&data_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// idle ストリームへの WINDOW_UPDATE は PROTOCOL_ERROR
    ///
    /// RFC 9113 Section 5.1: idle ストリームへの WINDOW_UPDATE は PROTOCOL_ERROR
    #[test]
    fn prop_window_update_on_idle_stream_is_error(stream_id in client_stream_id()) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // idle ストリームに WINDOW_UPDATE を送信
        let wu_frame = Frame::WindowUpdate(WindowUpdateFrame::for_stream(
            stream_id,
            WindowIncrement::from_static(1000),
        ));
        let wu_bytes = encode_frame(&wu_frame);
        server.feed(&wu_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// GOAWAY 受信後の新規ストリーム開始は PROTOCOL_ERROR
    ///
    /// RFC 9113 Section 6.8: GOAWAY 受信後は新規ストリームを開始できない
    #[test]
    fn prop_start_stream_after_goaway_is_error(_dummy in Just(())) {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // サーバーから SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        client.feed(&settings_bytes).unwrap();
        client.process().unwrap();

        // サーバーから GOAWAY を受信
        let goaway_frame = Frame::Goaway(GoawayFrame::new(
            LastStreamId::from_static(0),
            ErrorCode::NoError.as_u32(),
        ));
        let goaway_bytes = encode_frame(&goaway_frame);
        client.feed(&goaway_bytes).unwrap();
        client.process().unwrap();

        // GOAWAY 後に新規ストリームを開始しようとする
        use shiguredo_http2::HeaderField;
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
        ];
        let result = client.start_stream(headers, true);
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// INITIAL_WINDOW_SIZE の無効値は FLOW_CONTROL_ERROR
    ///
    /// RFC 9113 Section 6.5.2: INITIAL_WINDOW_SIZE の無効な値は FLOW_CONTROL_ERROR
    #[test]
    fn prop_invalid_initial_window_size_is_flow_control_error(
        invalid_size in (MAX_INITIAL_WINDOW_SIZE + 1)..=u32::MAX,
    ) {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // サーバーから無効な INITIAL_WINDOW_SIZE の SETTINGS を受信
        // decoder が Setting::from_wire で検証するため、raw バイト列を直接構築
        let mut settings_bytes = Vec::new();
        // フレームヘッダー: length=6, type=0x04 (SETTINGS), flags=0, stream_id=0
        settings_bytes.extend_from_slice(&[0x00, 0x00, 0x06, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
        // SETTINGS パラメータ: id=0x0004, value=invalid_size
        settings_bytes.extend_from_slice(&0x0004u16.to_be_bytes());
        settings_bytes.extend_from_slice(&invalid_size.to_be_bytes());
        client.feed(&settings_bytes).unwrap();

        let result = client.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::FlowControlError));
        }
    }

    /// 送信側の max_concurrent_streams チェック
    ///
    /// RFC 9113 Section 5.1.2: peer が設定した同時ストリーム上限を超えてはならない
    #[test]
    fn prop_start_stream_respects_remote_max_concurrent_streams(
        max_streams in 1u32..=5,
    ) {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // サーバーから max_concurrent_streams の SETTINGS を受信
        let mut settings = SettingsFrame::new();
        settings.add(Setting::MaxConcurrentStreams(max_streams));
        let settings_bytes = encode_frame(&Frame::Settings(settings));
        client.feed(&settings_bytes).unwrap();
        client.process().unwrap();

        // max_streams 個のストリームを開始（すべて成功するはず）
        for _ in 0..max_streams {
            let headers = vec![
                HeaderField::new(":method", "GET").unwrap(),
                HeaderField::new(":path", "/").unwrap(),
                HeaderField::new(":scheme", "https").unwrap(),
                HeaderField::new(":authority", "example.com").unwrap(),
            ];
            let result = client.start_stream(headers, false);
            prop_assert!(result.is_ok(), "stream should be started successfully");
        }

        // max_streams + 1 個目はエラーになるはず
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
        ];
        let result = client.start_stream(headers, false);
        prop_assert!(result.is_err(), "exceeding max concurrent streams should fail");
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

    /// NO_RFC7540_PRIORITIES の変更は PROTOCOL_ERROR
    ///
    /// RFC 9218 Section 2.1: この設定は接続中に変更できない
    #[test]
    fn prop_no_rfc7540_priorities_change_is_error(
        initial_value in prop::bool::ANY,
    ) {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // サーバーから NO_RFC7540_PRIORITIES の SETTINGS を受信
        let mut settings1 = SettingsFrame::new();
        settings1.add(Setting::NoRfc7540Priorities(initial_value));
        let settings1_bytes = encode_frame(&Frame::Settings(settings1));
        client.feed(&settings1_bytes).unwrap();
        client.process().unwrap();

        // サーバーから異なる値の NO_RFC7540_PRIORITIES を受信
        let mut settings2 = SettingsFrame::new();
        settings2.add(Setting::NoRfc7540Priorities(!initial_value));
        let settings2_bytes = encode_frame(&Frame::Settings(settings2));
        client.feed(&settings2_bytes).unwrap();

        let result = client.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// preface 未受信でフレーム処理がエラーになるテスト
    ///
    /// RFC 9113 Section 3.4: サーバーは client preface を受信済みでなければならない。
    /// feed() が接続プリフェイスを検証するため、不正なデータは feed() 時点でエラーになる。
    #[test]
    fn prop_server_rejects_frame_without_preface(_dummy in Just(())) {
        let mut server = Connection::server(Limits::default());
        // mark_preface_received() を呼ばずに initiate
        server.initiate().unwrap();

        // SETTINGS フレームを送信しても preface と一致しないためエラー
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        let result = server.feed(&settings_bytes);
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// preface 受信後は正常動作するテスト
    ///
    /// RFC 9113 Section 3.4: mark_preface_received() 後に SETTINGS 処理が成功する
    #[test]
    fn prop_server_accepts_frame_after_preface(_dummy in Just(())) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS フレームを送信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_ok());
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

    /// サーバー start_stream 禁止テスト
    ///
    /// RFC 9113 Section 8.4: サーバープッシュ非サポートのためサーバーは新規ストリームを開始できない
    #[test]
    fn prop_server_cannot_start_stream(_dummy in Just(())) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信して接続をアクティブにする
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
        ];
        let result = server.start_stream(headers, true);
        prop_assert!(result.is_err());
    }

    // ========================================================================
    // 双方向通信シナリオの PBT
    // ========================================================================
}

/// クライアントとサーバー間のハンドシェイクを完了する
fn setup_client_server() -> (Connection, Connection) {
    let mut client = Connection::client(Limits::default());
    let mut server = Connection::server(Limits::default());

    // クライアント: プリフェイスと SETTINGS を送信
    client.initiate().unwrap();
    let client_output = client.poll_output().unwrap();

    // サーバー: クライアントのプリフェイスを受信
    server.mark_preface_received();
    server.initiate().unwrap();
    let settings_start = shiguredo_http2::CONNECTION_PREFACE_LEN;
    server.feed(&client_output[settings_start..]).unwrap();
    server.process().unwrap();

    // サーバー: イベントを消費
    while server.poll_event().is_some() {}

    // サーバー: SETTINGS + ACK を送信
    let server_output = server.poll_output().unwrap();

    // クライアント: サーバーの SETTINGS を受信
    client.feed(&server_output).unwrap();
    client.process().unwrap();

    // クライアント: イベントを消費
    while client.poll_event().is_some() {}

    (client, server)
}

proptest! {
    /// クライアント-サーバー間の正常なリクエスト/レスポンスサイクル
    ///
    /// RFC 9113 Section 8.1: 正常な HTTP/2 リクエスト/レスポンスの流れ
    #[test]
    fn prop_request_response_cycle(
        path in "/[a-z]{1,10}",
    ) {
        let (mut client, mut server) = setup_client_server();

        // クライアント: リクエストを送信
        let request_headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", &path).unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
        ];
        let stream_id = client.start_stream(request_headers, true).unwrap();
        prop_assert_eq!(stream_id, StreamId::from_wire(1)); // 最初のクライアントストリーム

        // クライアントの出力をサーバーに送信
        if let Some(client_output) = client.poll_output() {
            server.feed(&client_output).unwrap();
            server.process().unwrap();
        }

        // サーバー: HeadersReceived イベントを確認
        let mut found_headers = false;
        while let Some(event) = server.poll_event() {
            if matches!(
                &event,
                shiguredo_http2::Event::HeadersReceived { stream_id, end_stream: true, .. }
                    if stream_id.as_u32() == 1
            ) {
                found_headers = true;
                break;
            }
        }
        prop_assert!(found_headers, "expected HeadersReceived event");
    }

    /// 複数ストリームの並行処理
    ///
    /// RFC 9113 Section 5.1.2: 複数のストリームを並行して処理できる
    #[test]
    fn prop_concurrent_streams(
        count in 1..5usize,
    ) {
        let (mut client, _server) = setup_client_server();

        // 複数のストリームを開く
        let mut stream_ids = Vec::new();
        for i in 0..count {
            let request_headers = vec![
                HeaderField::new(":method", "GET").unwrap(),
                HeaderField::new(":scheme", "https").unwrap(),
                HeaderField::new(":path", format!("/resource{}", i)).unwrap(),
                HeaderField::new(":authority", "example.com").unwrap(),
            ];
            let stream_id = client.start_stream(request_headers, true).unwrap();
            stream_ids.push(stream_id);
        }

        // ストリーム ID は奇数で単調増加
        for (i, &id) in stream_ids.iter().enumerate() {
            let expected = StreamId::from_wire(i as u32 * 2 + 1); // 1, 3, 5, ...
            prop_assert_eq!(id, expected);
        }
    }

    /// PING フレームのエコー
    ///
    /// RFC 9113 Section 6.7: PING フレームは ACK でエコーされる
    #[test]
    fn prop_ping_echo(opaque_data in any::<[u8; 8]>()) {
        let (_client, mut server) = setup_client_server();

        // クライアント: PING を送信
        let ping_frame = Frame::Ping(PingFrame::new(opaque_data));
        let ping_bytes = encode_frame(&ping_frame);
        server.feed(&ping_bytes).unwrap();
        server.process().unwrap();

        // サーバー: PingReceived イベントを確認
        let mut found_ping = false;
        while let Some(event) = server.poll_event() {
            if matches!(&event, shiguredo_http2::Event::PingReceived { ack: false, .. }) {
                found_ping = true;
                break;
            }
        }
        prop_assert!(found_ping, "expected PingReceived event");

        // サーバー: PING ACK を送信
        let server_output = server.poll_output().unwrap();
        prop_assert!(!server_output.is_empty());
    }

    /// WINDOW_UPDATE による送信ウィンドウの増加
    ///
    /// RFC 9113 Section 6.9: WINDOW_UPDATE でフロー制御ウィンドウを増加させる
    #[test]
    fn prop_window_update_increases_window(increment in 1u32..=0x7FFF_FFFF) {
        let (_client, mut server) = setup_client_server();

        // クライアント: 接続レベルの WINDOW_UPDATE を送信
        let wi = WindowIncrement::new(increment).expect("1..=0x7FFF_FFFF is valid");
        let wu_frame = Frame::WindowUpdate(WindowUpdateFrame::for_connection(wi));
        let wu_bytes = encode_frame(&wu_frame);
        server.feed(&wu_bytes).unwrap();
        server.process().unwrap();

        // サーバー: WindowUpdateReceived イベントを確認
        let mut found_window_update = false;
        while let Some(event) = server.poll_event() {
            if matches!(&event, shiguredo_http2::Event::WindowUpdateReceived { stream_id: StreamId::Connection, .. }) {
                found_window_update = true;
                break;
            }
        }
        prop_assert!(found_window_update, "expected WindowUpdateReceived event");
    }

    /// GOAWAY による正常終了
    ///
    /// RFC 9113 Section 6.8: GOAWAY で接続を正常終了する
    #[test]
    fn prop_goaway_graceful_shutdown(_dummy in Just(())) {
        let (mut client, _server) = setup_client_server();

        // サーバー: GOAWAY を送信
        let goaway_frame = Frame::Goaway(GoawayFrame::new(
            LastStreamId::from_static(0),
            ErrorCode::NoError.as_u32(),
        ));
        let goaway_bytes = encode_frame(&goaway_frame);
        client.feed(&goaway_bytes).unwrap();
        client.process().unwrap();

        // クライアント: GoawayReceived イベントを確認
        let mut found_goaway = false;
        while let Some(event) = client.poll_event() {
            if matches!(&event, shiguredo_http2::Event::GoawayReceived { error_code: ErrorCode::NoError, .. }) {
                found_goaway = true;
                break;
            }
        }
        prop_assert!(found_goaway, "expected GoawayReceived event");
    }

    /// RST_STREAM によるストリームキャンセル
    ///
    /// RFC 9113 Section 6.4: RST_STREAM でストリームを即座に終了
    #[test]
    fn prop_rst_stream_cancels_stream(error_code in any::<u32>()) {
        let (mut client, mut server) = setup_client_server();

        // クライアント: リクエストを送信 (END_STREAM なし)
        let request_headers = vec![
            HeaderField::new(":method", "POST").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
        ];
        client.start_stream(request_headers, false).unwrap();
        let client_output = client.poll_output().unwrap();
        server.feed(&client_output).unwrap();
        server.process().unwrap();
        // サーバー: イベントを消費
        while server.poll_event().is_some() {}

        // サーバー: RST_STREAM を送信
        let rst_frame = Frame::RstStream(RstStreamFrame::new(
            NonZeroStreamId::from_static(1),
            error_code,
        ));
        let rst_bytes = encode_frame(&rst_frame);
        client.feed(&rst_bytes).unwrap();
        client.process().unwrap();

        // クライアント: StreamReset イベントを確認
        let mut found_reset = false;
        while let Some(event) = client.poll_event() {
            if matches!(&event, shiguredo_http2::Event::StreamReset { stream_id, .. } if stream_id.as_u32() == 1) {
                found_reset = true;
                break;
            }
        }
        prop_assert!(found_reset, "expected StreamReset event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
