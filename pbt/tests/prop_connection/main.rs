//! HTTP/2 接続の PBT — 接続レベルテスト
//!
//! RFC 9113 準拠の接続レベル検証をテストする。

mod headers;
mod settings;

use proptest::prelude::*;
use shiguredo_http2::{
    Connection, HeaderField, Limits, NonZeroStreamId, WindowIncrement, WindowSize,
    frame::{
        Frame, FrameDecoder, FrameEncoder, PingFrame, RstStreamFrame, SettingsFrame, StreamId,
        WindowUpdateFrame,
    },
    settings::{DEFAULT_INITIAL_WINDOW_SIZE, MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, Setting},
};

/// 有効なストリーム ID を生成する（クライアント開始: 奇数）
pub(crate) fn client_stream_id() -> impl Strategy<Value = NonZeroStreamId> {
    (1u32..=100).prop_map(|n| NonZeroStreamId::new(n * 2 + 1).expect("odd value is always valid"))
}

/// フレームをバイト列にエンコードする
pub(crate) fn encode_frame(frame: &Frame) -> Vec<u8> {
    let mut encoder = FrameEncoder::new();
    encoder.encode(frame).unwrap();
    encoder.buffer().to_vec()
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

    // ========================================================================
    // 双方向通信シナリオの PBT
    // ========================================================================
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
    fn prop_window_update_increases_window(
        // 初期ウィンドウサイズ (65535) との合計が 2^31-1 を超えないよう上限を制限する
        // (RFC 9113 §6.9.1: 上限超過は FLOW_CONTROL_ERROR)
        increment in 1u32..=(0x7FFF_FFFFu32 - 65535),
    ) {
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

proptest! {
    // ========================================================================
    // 接続レベル WINDOW_UPDATE の広告 (issue 0041) の PBT
    // ========================================================================

    /// `connection_window_size` がデフォルトより大きい場合、`initiate()` の出力に
    /// `connection_window_size - DEFAULT_INITIAL_WINDOW_SIZE` の WINDOW_UPDATE が含まれる
    ///
    /// RFC 9113 Section 6.9.2: 接続レベルのフロー制御ウィンドウは WINDOW_UPDATE でのみ
    /// 拡張できる。SETTINGS_INITIAL_WINDOW_SIZE は接続レベルに適用されない。
    #[test]
    fn prop_initiate_emits_connection_window_update(
        size in (DEFAULT_INITIAL_WINDOW_SIZE + 1)..=MAX_INITIAL_WINDOW_SIZE,
    ) {
        let window = WindowSize::from_static(size);
        let limits = Limits::builder()
            .connection_window_size(window)
            .build()
            .expect("valid limits");
        let mut client = Connection::client(limits);
        client.initiate().expect("initiate");

        let output = client.poll_output().expect("output must contain preface + settings");

        // CONNECTION_PREFACE をスキップしてから FrameDecoder にかける
        let preface_len = shiguredo_http2::CONNECTION_PREFACE_LEN;
        prop_assert!(output.len() > preface_len);
        let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
        decoder.feed(&output[preface_len..]);

        let mut found = None;
        while let Some(frame) = decoder.decode().expect("decode frame") {
            if let Frame::WindowUpdate(wu) = frame
                && matches!(wu.stream_id, StreamId::Connection)
            {
                found = Some(wu.window_size_increment.as_u32());
            }
        }
        let expected = size - DEFAULT_INITIAL_WINDOW_SIZE;
        prop_assert_eq!(
            found,
            Some(expected),
            "expected connection-level WINDOW_UPDATE with increment {}",
            expected
        );
    }
}
