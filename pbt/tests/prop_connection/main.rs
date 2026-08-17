//! HTTP/2 接続の PBT — 接続レベルテスト
//!
//! RFC 9113 準拠の接続レベル検証をテストする。

mod headers;
mod settings;

use shiguredo_http2::{
    Connection, HeaderField, Limits, NonZeroStreamId, WindowIncrement, WindowSize,
    frame::{
        Frame, FrameDecoder, FrameEncoder, PingFrame, RstStreamFrame, SettingsFrame, StreamId,
        WindowUpdateFrame,
    },
    settings::{DEFAULT_INITIAL_WINDOW_SIZE, MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, Setting},
};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// 有効なストリーム ID (クライアント開始: 奇数) を生成する
pub(crate) fn sample_client_stream_id(ctx: &mut noprop::TestCaseContext) -> NonZeroStreamId {
    let n = 1 + noprop::sample_usize_in(ctx, 0..=99);
    NonZeroStreamId::new(n as u32 * 2 + 1).expect("odd value is always valid")
}

/// フレームをバイト列にエンコードする
pub(crate) fn encode_frame(frame: &Frame) -> Vec<u8> {
    let mut encoder = FrameEncoder::new();
    encoder.encode(frame).expect("encode should succeed");
    encoder.buffer().to_vec()
}

/// クライアントとサーバー間のハンドシェイクを完了する
fn setup_client_server() -> (Connection, Connection) {
    let mut client = Connection::client(Limits::default());
    let mut server = Connection::server(Limits::default());

    // クライアント: プリフェイスと SETTINGS を送信
    client.initiate().expect("initiate should succeed");
    let client_output = client.poll_output().expect("initiate should succeed");

    // サーバー: クライアントのプリフェイスを受信
    server.mark_preface_received();
    server.initiate().expect("initiate should succeed");
    let settings_start = shiguredo_http2::CONNECTION_PREFACE_LEN;
    server
        .feed(&client_output[settings_start..])
        .expect("feed should succeed");
    server.process().expect("process should succeed");

    // サーバー: イベントを消費
    while server.poll_event().is_some() {}

    // サーバー: SETTINGS + ACK を送信
    let server_output = server.poll_output().expect("should succeed");

    // クライアント: サーバーの SETTINGS を受信
    client.feed(&server_output).expect("feed should succeed");
    client.process().expect("process should succeed");

    // クライアント: イベントを消費
    while client.poll_event().is_some() {}

    (client, server)
}

/// 送信側の max_concurrent_streams チェック
///
/// RFC 9113 Section 5.1.2: peer が設定した同時ストリーム上限を超えてはならない
#[test]
fn prop_start_stream_respects_remote_max_concurrent_streams() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_streams = noprop::sample_u64_in(ctx, 1..=5) as u32;
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate should succeed");

        // サーバーから max_concurrent_streams の SETTINGS を受信
        let mut settings = SettingsFrame::new();
        settings.add(Setting::MaxConcurrentStreams(max_streams));
        let settings_bytes = encode_frame(&Frame::Settings(settings));
        client.feed(&settings_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        // max_streams 個のストリームを開始（すべて成功するはず）
        for _ in 0..max_streams {
            let headers = vec![
                HeaderField::new(":method", "GET").expect("valid header field"),
                HeaderField::new(":path", "/").expect("valid header field"),
                HeaderField::new(":scheme", "https").expect("valid header field"),
                HeaderField::new(":authority", "example.com").expect("valid header field"),
            ];
            let result = client.start_stream(headers, false);
            assert!(
                result.is_ok(),
                "stream should be started successfully (max_streams={max_streams})"
            );
        }

        // max_streams + 1 個目はエラーになるはず
        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
        ];
        let result = client.start_stream(headers, false);
        assert!(
            result.is_err(),
            "exceeding max concurrent streams should fail"
        );
        Ok(())
    })?;
    Ok(())
}

/// クライアント-サーバー間の正常なリクエスト/レスポンスサイクル
///
/// RFC 9113 Section 8.1: 正常な HTTP/2 リクエスト/レスポンスの流れ
#[test]
fn prop_request_response_cycle() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let path_len = noprop::sample_usize_in(ctx, 1..=10);
        let mut path = String::from("/");
        for _ in 0..path_len {
            path.push((b'a' + noprop::sample_usize_in(ctx, 0..26) as u8) as char);
        }
        let (mut client, mut server) = setup_client_server();

        // クライアント: リクエストを送信
        let request_headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", &path).expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
        ];
        let stream_id = client
            .start_stream(request_headers, true)
            .expect("should succeed");
        assert_eq!(stream_id, StreamId::from_wire(1)); // 最初のクライアントストリーム

        // クライアントの出力をサーバーに送信
        if let Some(client_output) = client.poll_output() {
            server.feed(&client_output).expect("feed should succeed");
            server.process().expect("process should succeed");
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
        assert!(found_headers, "expected HeadersReceived event");
        Ok(())
    })?;
    Ok(())
}

/// 複数ストリームの並行処理
///
/// RFC 9113 Section 5.1.2: 複数のストリームを並行して処理できる
#[test]
fn prop_concurrent_streams() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let count = noprop::sample_usize_in(ctx, 1..=4);
        let (mut client, _server) = setup_client_server();

        // 複数のストリームを開く
        let mut stream_ids = Vec::new();
        for i in 0..count {
            let request_headers = vec![
                HeaderField::new(":method", "GET").expect("valid header field"),
                HeaderField::new(":scheme", "https").expect("valid header field"),
                HeaderField::new(":path", format!("/resource{}", i)).expect("valid header field"),
                HeaderField::new(":authority", "example.com").expect("valid header field"),
            ];
            let stream_id = client
                .start_stream(request_headers, true)
                .expect("should succeed");
            stream_ids.push(stream_id);
        }

        // ストリーム ID は奇数で単調増加
        for (i, &id) in stream_ids.iter().enumerate() {
            let expected = StreamId::from_wire(i as u32 * 2 + 1); // 1, 3, 5, ...
            assert_eq!(id, expected);
        }
        Ok(())
    })?;
    Ok(())
}

/// PING フレームのエコー
///
/// RFC 9113 Section 6.7: PING フレームは ACK でエコーされる
#[test]
fn prop_ping_echo() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let opaque_data = noprop::sample_bytes::<8>(ctx);
        let (_client, mut server) = setup_client_server();

        // クライアント: PING を送信
        let ping_frame = Frame::Ping(PingFrame::new(opaque_data));
        let ping_bytes = encode_frame(&ping_frame);
        server.feed(&ping_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        // サーバー: PingReceived イベントを確認
        let mut found_ping = false;
        while let Some(event) = server.poll_event() {
            if matches!(
                &event,
                shiguredo_http2::Event::PingReceived { ack: false, .. }
            ) {
                found_ping = true;
                break;
            }
        }
        assert!(found_ping, "expected PingReceived event");

        // サーバー: PING ACK を送信
        let server_output = server.poll_output().expect("should succeed");
        assert!(!server_output.is_empty());
        Ok(())
    })?;
    Ok(())
}

/// WINDOW_UPDATE による送信ウィンドウの増加
///
/// RFC 9113 Section 6.9: WINDOW_UPDATE でフロー制御ウィンドウを増加させる
#[test]
fn prop_window_update_increases_window() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 初期ウィンドウサイズ (65535) との合計が 2^31-1 を超えないよう上限を制限する
        // (RFC 9113 §6.9.1: 上限超過は FLOW_CONTROL_ERROR)
        let increment = noprop::sample_u64_in(ctx, 1..=(0x7FFF_FFFF - 65535) as u64) as u32;
        let (_client, mut server) = setup_client_server();

        // クライアント: 接続レベルの WINDOW_UPDATE を送信
        let wi = WindowIncrement::new(increment).expect("1..=0x7FFF_FFFF is valid");
        let wu_frame = Frame::WindowUpdate(WindowUpdateFrame::for_connection(wi));
        let wu_bytes = encode_frame(&wu_frame);
        server.feed(&wu_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        // サーバー: WindowUpdateReceived イベントを確認
        let mut found_window_update = false;
        while let Some(event) = server.poll_event() {
            if matches!(
                &event,
                shiguredo_http2::Event::WindowUpdateReceived {
                    stream_id: StreamId::Connection,
                    ..
                }
            ) {
                found_window_update = true;
                break;
            }
        }
        assert!(found_window_update, "expected WindowUpdateReceived event");
        Ok(())
    })?;
    Ok(())
}

/// RST_STREAM によるストリームキャンセル
///
/// RFC 9113 Section 6.4: RST_STREAM でストリームを即座に終了
#[test]
fn prop_rst_stream_cancels_stream() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let error_code = noprop::sample_u32(ctx);
        let (mut client, mut server) = setup_client_server();

        // クライアント: リクエストを送信 (END_STREAM なし)
        let request_headers = vec![
            HeaderField::new(":method", "POST").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
        ];
        client.start_stream(request_headers, false).expect("should succeed");
        let client_output = client.poll_output().expect("should succeed");
        server.feed(&client_output).expect("feed should succeed");
        server.process().expect("process should succeed");
        // サーバー: イベントを消費
        while server.poll_event().is_some() {}

        // サーバー: RST_STREAM を送信
        let rst_frame = Frame::RstStream(RstStreamFrame::new(
            NonZeroStreamId::from_static(1),
            error_code,
        ));
        let rst_bytes = encode_frame(&rst_frame);
        client.feed(&rst_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        // クライアント: StreamReset イベントを確認
        let mut found_reset = false;
        while let Some(event) = client.poll_event() {
            if matches!(&event, shiguredo_http2::Event::StreamReset { stream_id, .. } if stream_id.as_u32() == 1) {
                found_reset = true;
                break;
            }
        }
        assert!(found_reset, "expected StreamReset event");
        Ok(())
    })?;
    Ok(())
}

/// `connection_window_size` がデフォルトより大きい場合、`initiate()` の出力に
/// `connection_window_size - DEFAULT_INITIAL_WINDOW_SIZE` の WINDOW_UPDATE が含まれる
///
/// RFC 9113 Section 6.9.2: 接続レベルのフロー制御ウィンドウは WINDOW_UPDATE でのみ
/// 拡張できる。SETTINGS_INITIAL_WINDOW_SIZE は接続レベルに適用されない。
#[test]
fn prop_initiate_emits_connection_window_update() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let size = (DEFAULT_INITIAL_WINDOW_SIZE + 1)
            + noprop::sample_u64_in(
                ctx,
                0..=(MAX_INITIAL_WINDOW_SIZE - DEFAULT_INITIAL_WINDOW_SIZE) as u64,
            ) as u32;
        let window = WindowSize::from_static(size);
        let limits = Limits::builder()
            .connection_window_size(window)
            .build()
            .expect("valid limits");
        let mut client = Connection::client(limits);
        client.initiate().expect("initiate");

        let output = client
            .poll_output()
            .expect("output must contain preface + settings");

        // CONNECTION_PREFACE をスキップしてから FrameDecoder にかける
        let preface_len = shiguredo_http2::CONNECTION_PREFACE_LEN;
        assert!(output.len() > preface_len);
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
        assert_eq!(
            found,
            Some(expected),
            "expected connection-level WINDOW_UPDATE with increment {expected}",
        );
        Ok(())
    })?;
    Ok(())
}
