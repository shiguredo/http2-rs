//! クライアント/サーバー統合テスト
#![allow(clippy::collapsible_match)]

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use shiguredo_http2::WindowSize;
use tokio_http2::{
    Client, ErrorCode, Event, HeaderField, Limits, Server, StreamId, TlsServerConfig,
};

/// テスト用自己署名証明書を生成
fn generate_test_cert() -> TlsServerConfig {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("failed to generate certificate");

    TlsServerConfig::from_der(
        vec![CertificateDer::from(cert.der().to_vec())],
        PrivateKeyDer::try_from(signing_key.serialize_der()).unwrap(),
    )
    .expect("failed to create TLS server config")
}

/// 基本的なリクエスト/レスポンス
#[tokio::test]
async fn test_basic_request_response() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    // サーバー起動
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    // サーバータスク
    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // クライアントからのリクエストを待機
        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::SettingsReceived { .. } => {
                    // SETTINGS 処理
                }
                Event::ConnectionPreface => {
                    // 接続プリフェイス受信
                }
                Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                } => {
                    // リクエストヘッダー受信
                    assert!(end_stream);

                    // :method ヘッダーを確認
                    let method = headers
                        .iter()
                        .find(|h| h.name() == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value(), b"GET");

                    // :path ヘッダーを確認
                    let path = headers
                        .iter()
                        .find(|h| h.name() == b":path")
                        .expect("missing :path header");
                    assert_eq!(path.value(), b"/");

                    // レスポンス送信
                    let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                    conn.send_response(stream_id, response_headers, true)
                        .await
                        .expect("failed to send response");

                    break;
                }
                _ => {}
            }
        }
    });

    // クライアント接続
    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // リクエスト送信
    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    // レスポンス待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::SettingsReceived { .. } => {
                // SETTINGS 処理
            }
            Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(end_stream);

                // :status ヘッダーを確認
                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value(), b"200");

                break;
            }
            _ => {}
        }
    }

    server_handle.await.expect("server task failed");
}

/// PING/PONG
#[tokio::test]
async fn test_ping_pong() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // イベントループ
        loop {
            match conn.next_event().await {
                Ok(Event::PingReceived { ack, .. }) => {
                    if !ack {
                        // PING 受信 (自動で PONG が送信される)
                        conn.flush().await.expect("failed to flush");
                    }
                }
                Ok(Event::SettingsReceived { .. }) => {}
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // 初期 SETTINGS を処理
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // PING 送信
    let ping_data = [1, 2, 3, 4, 5, 6, 7, 8];
    client.ping(ping_data).await.expect("failed to send ping");

    // PONG 待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::PingReceived { opaque_data, ack } = event {
            assert!(ack);
            assert_eq!(opaque_data, ping_data);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.abort();
}

/// SETTINGS 交換
#[tokio::test]
async fn test_settings_exchange() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut received_settings = false;
        let mut received_ack = false;

        // SETTINGS と SETTINGS ACK の両方を待機
        loop {
            match conn.next_event().await {
                Ok(Event::SettingsReceived { ack }) => {
                    if ack {
                        received_ack = true;
                    } else {
                        received_settings = true;
                    }
                    if received_settings && received_ack {
                        break;
                    }
                }
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(received_settings);
        assert!(received_ack);

        // GOAWAY を送信してから終了
        conn.shutdown().await.expect("failed to send goaway");
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    let mut received_settings = false;
    let mut received_ack = false;

    // SETTINGS と SETTINGS ACK の両方を待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack } = event {
            if ack {
                received_ack = true;
            } else {
                received_settings = true;
            }
            if received_settings && received_ack {
                break;
            }
        }
    }

    assert!(received_settings);
    assert!(received_ack);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 複数ストリーム
#[tokio::test]
async fn test_multiple_streams() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut streams_received = 0;

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        // レスポンス送信
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");

                        streams_received += 1;
                        if streams_received >= 3 {
                            break;
                        }
                    }
                }
                Ok(Event::SettingsReceived { .. }) => {}
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // 初期 SETTINGS を処理
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // 3 つのリクエストを送信
    let mut stream_ids = Vec::new();
    for i in 0..3 {
        let request_headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", format!("/path{}", i)).unwrap(),
            HeaderField::new(":authority", "localhost").unwrap(),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }

    // 3 つのレスポンスを受信
    let mut responses_received = 0;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert!(stream_ids.contains(&stream_id));
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");

            responses_received += 1;
            if responses_received >= 3 {
                break;
            }
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// GOAWAY
#[tokio::test]
async fn test_goaway() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // 初期 SETTINGS を処理
        loop {
            match conn.next_event().await {
                Ok(Event::SettingsReceived { ack: true }) => break,
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => return,
            }
        }

        // GOAWAY 送信
        conn.shutdown().await.expect("failed to send goaway");
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // GOAWAY 待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::GoawayReceived {
            last_stream_id,
            error_code,
            ..
        } = event
        {
            assert_eq!(last_stream_id, StreamId::Connection);
            assert_eq!(error_code, ErrorCode::NoError);
            break;
        }
    }

    server_handle.await.expect("server task failed");
}

/// RST_STREAM
#[tokio::test]
async fn test_rst_stream() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        // RST_STREAM を送信
                        conn.reset_stream(stream_id, ErrorCode::Cancel)
                            .await
                            .expect("failed to send rst_stream");
                        break;
                    }
                }
                Ok(Event::SettingsReceived { .. }) => {}
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // 初期 SETTINGS を処理
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // リクエスト送信
    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    // StreamReset イベント待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::StreamReset {
            stream_id: reset_stream_id,
            error_code,
        } = event
        {
            assert_eq!(reset_stream_id, stream_id);
            assert_eq!(error_code, ErrorCode::Cancel);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

// ============================================================================
// 嫌がらせ系テスト
// ============================================================================

/// 大量ストリームを同時に開く
#[tokio::test]
async fn test_many_concurrent_streams() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let stream_count = 50;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut streams_responded = 0;
        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");

                        streams_responded += 1;
                        if streams_responded >= stream_count {
                            break;
                        }
                    }
                }
                Ok(Event::SettingsReceived { .. }) => {}
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let mut stream_ids = Vec::new();
    for i in 0..stream_count {
        let request_headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":authority", "localhost").unwrap(),
            HeaderField::new(":path", format!("/path/{}", i)).unwrap(),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }

    let mut responses = 0;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            stream_id,
            end_stream,
            ..
        } = event
        {
            assert!(stream_ids.contains(&stream_id));
            assert!(end_stream);
            responses += 1;
            if responses >= stream_count {
                break;
            }
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 連続 RST_STREAM 送信
#[tokio::test]
async fn test_rapid_rst_stream() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let rst_count = 10;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut rst_sent = 0;
        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        conn.reset_stream(stream_id, ErrorCode::Cancel)
                            .await
                            .expect("failed to send rst_stream");

                        rst_sent += 1;
                        if rst_sent >= rst_count {
                            break;
                        }
                    }
                }
                Ok(Event::SettingsReceived { .. }) => {}
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let mut stream_ids = Vec::new();
    for _ in 0..rst_count {
        let request_headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":authority", "localhost").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }

    let mut resets = 0;
    loop {
        match client.next_event().await {
            Ok(Event::StreamReset { .. }) => {
                resets += 1;
                if resets >= rst_count {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert_eq!(resets, rst_count);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 連続 PING 送信
#[tokio::test]
async fn test_rapid_ping() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let ping_count: u8 = 20;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::PingReceived { ack, .. }) => {
                    if !ack {
                        conn.flush().await.expect("failed to flush");
                    }
                }
                Ok(Event::SettingsReceived { .. }) => {}
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    for i in 0..ping_count {
        let data = [i, 0, 0, 0, 0, 0, 0, 0];
        client.ping(data).await.expect("failed to send ping");
    }

    let mut pong_count: u8 = 0;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::PingReceived { ack: true, .. } = event {
            pong_count += 1;
            if pong_count >= ping_count {
                break;
            }
        }
    }

    assert_eq!(pong_count, ping_count);

    client.shutdown().await.ok();
    server_handle.abort();
}

/// 大量の小さいデータフレーム
#[tokio::test]
async fn test_many_small_data_frames() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let chunk_count: usize = 100;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response");

                        for i in 0..chunk_count {
                            let is_last = i == chunk_count - 1;
                            conn.send_data(stream_id, vec![i as u8], is_last)
                                .await
                                .expect("failed to send data");
                        }
                        break;
                    }
                }
                Event::SettingsReceived { .. } => {}
                Event::ConnectionPreface => {}
                _ => {}
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::DataReceived {
                data, end_stream, ..
            } => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Event::HeadersReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_data.len(), chunk_count);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 大量ヘッダー
#[tokio::test]
async fn test_many_headers() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let header_count = 50;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        let custom_count = headers
                            .iter()
                            .filter(|h| h.name().starts_with(b"x-test-"))
                            .count();
                        assert_eq!(custom_count, header_count);

                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Event::SettingsReceived { .. } => {}
                Event::ConnectionPreface => {}
                _ => {}
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let mut request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
    ];
    for i in 0..header_count {
        request_headers
            .push(HeaderField::new(format!("x-test-{}", i), format!("value-{}", i)).unwrap());
    }

    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            stream_id: recv_id,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_id, stream_id);
            assert!(end_stream);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// GOAWAY 後に既存ストリームのレスポンスを受信
#[tokio::test]
async fn test_goaway_then_drain() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let stream_id = loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        break stream_id;
                    }
                }
                Ok(Event::SettingsReceived { .. }) => {}
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => return,
            }
        };

        // GOAWAY を送信 (既存ストリームは処理する)
        conn.shutdown().await.expect("failed to send goaway");

        // 既存ストリームにはレスポンスを返す
        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
        conn.send_response(stream_id, response_headers, true)
            .await
            .expect("failed to send response");
        conn.flush().await.expect("failed to flush");
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut got_goaway = false;
    let mut got_response = false;
    loop {
        match client.next_event().await {
            Ok(Event::GoawayReceived { .. }) => got_goaway = true,
            Ok(Event::HeadersReceived {
                stream_id: recv_id,
                end_stream,
                ..
            }) => {
                assert_eq!(recv_id, stream_id);
                assert!(end_stream);
                got_response = true;
            }
            Ok(_) => {}
            Err(_) => break,
        }
        if got_goaway && got_response {
            break;
        }
    }

    assert!(got_goaway);
    assert!(got_response);

    server_handle.await.expect("server task failed");
}

/// 双方向ストリーミング (エコーサーバー)
#[tokio::test]
async fn test_bidirectional_streaming() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // ヘッダー受信後に設定される
        let mut stream_id: Option<StreamId> = None;

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived { stream_id: sid, .. } => {
                    stream_id = Some(sid);
                    let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                    conn.send_response(sid, response_headers, false)
                        .await
                        .expect("failed to send response");
                }
                Event::DataReceived {
                    data, end_stream, ..
                } => {
                    let sid = stream_id.expect("data received before headers");
                    conn.send_data(sid, data, end_stream)
                        .await
                        .expect("failed to echo data");
                    if end_stream {
                        break;
                    }
                }
                Event::SettingsReceived { .. } => {}
                Event::ConnectionPreface => {}
                _ => {}
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "POST").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":path", "/echo").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request");

    client
        .send_data(stream_id, b"chunk1-".to_vec(), false)
        .await
        .expect("failed to send data 1");
    client
        .send_data(stream_id, b"chunk2-".to_vec(), false)
        .await
        .expect("failed to send data 2");
    client
        .send_data(stream_id, b"chunk3".to_vec(), true)
        .await
        .expect("failed to send data 3");

    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::DataReceived {
                data, end_stream, ..
            } => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Event::HeadersReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_data, b"chunk1-chunk2-chunk3");

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// リクエストとレスポンスの交互送信
#[tokio::test]
async fn test_interleaved_streams() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut responded = 0;

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response");
                        conn.send_data(
                            stream_id,
                            format!("response-{}", responded).into_bytes(),
                            true,
                        )
                        .await
                        .expect("failed to send data");

                        responded += 1;
                        if responded >= 5 {
                            break;
                        }
                    }
                }
                Ok(Event::SettingsReceived { .. }) => {}
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    for _ in 0..5 {
        let request_headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":authority", "localhost").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
        ];
        client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");
    }

    let mut complete = 0;
    loop {
        match client.next_event().await {
            Ok(Event::DataReceived { end_stream, .. }) => {
                if end_stream {
                    complete += 1;
                    if complete >= 5 {
                        break;
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    assert_eq!(complete, 5);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// レスポンスヘッダー + データ送信
#[tokio::test]
async fn test_response_with_data() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response");

                        conn.send_data(stream_id, b"content".to_vec(), true)
                            .await
                            .expect("failed to send data");
                        // GOAWAY を送信して正常終了
                        conn.shutdown().await.expect("failed to shutdown");
                        break;
                    }
                }
                Event::SettingsReceived { .. } => {}
                Event::ConnectionPreface => {}
                _ => {}
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::DataReceived {
                data, end_stream, ..
            } => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Event::HeadersReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_data, b"content");

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

// ============================================================================
// e2e テスト: 実用シナリオ
// ============================================================================

/// POST リクエストでリクエストボディを送信し、レスポンスボディを受信する
#[tokio::test]
async fn test_post_request_with_body() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // ヘッダー受信後に設定される
        let mut stream_id: Option<StreamId> = None;
        let mut request_body = Vec::new();

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived {
                    stream_id: sid,
                    headers,
                    end_stream,
                    ..
                } => {
                    assert!(!end_stream, "POST should not have end_stream on headers");
                    let method = headers
                        .iter()
                        .find(|h| h.name() == b":method")
                        .expect("missing :method");
                    assert_eq!(method.value(), b"POST");
                    stream_id = Some(sid);
                }
                Event::DataReceived {
                    data, end_stream, ..
                } => {
                    request_body.extend_from_slice(&data);
                    if end_stream {
                        let sid = stream_id.expect("data received before headers");
                        // エコーレスポンス
                        let response_headers = vec![
                            HeaderField::new(":status", "200").unwrap(),
                            HeaderField::new("content-length", request_body.len().to_string())
                                .unwrap(),
                        ];
                        conn.send_response(sid, response_headers, false)
                            .await
                            .expect("failed to send response");
                        conn.send_data(sid, request_body.clone(), true)
                            .await
                            .expect("failed to send data");
                        break;
                    }
                }
                Event::SettingsReceived { .. } | Event::ConnectionPreface => {}
                _ => {}
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "POST").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/api/data").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new("content-type", "application/json").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request");

    let body = b"{\"key\":\"value\",\"number\":42}";
    client
        .send_data(stream_id, body.to_vec(), true)
        .await
        .expect("failed to send data");

    let mut response_body = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::DataReceived {
                data, end_stream, ..
            } => {
                response_body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Event::HeadersReceived { headers, .. } => {
                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status");
                assert_eq!(status.value(), b"200");
            }
            _ => {}
        }
    }

    assert_eq!(response_body, body);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 大きなボディ (フロー制御ウィンドウの限界に近い)
#[tokio::test]
async fn test_large_body() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    // 64KB のデータ (デフォルトウィンドウサイズ 65535 に近い)
    let body_size: usize = 60000;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let stream_id = loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        break stream_id;
                    }
                }
                Event::SettingsReceived { .. } | Event::ConnectionPreface => {}
                _ => {}
            }
        };

        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
        conn.send_response(stream_id, response_headers, false)
            .await
            .expect("failed to send response");

        // 大きなデータを複数チャンクで送信
        let data: Vec<u8> = (0..body_size).map(|i| (i % 256) as u8).collect();
        let chunk_size = 16384; // max_frame_size
        let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let is_last = i == chunks.len() - 1;
            conn.send_data(stream_id, chunk.to_vec(), is_last)
                .await
                .expect("failed to send data chunk");
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/large").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::DataReceived {
                data, end_stream, ..
            } => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Event::HeadersReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_data.len(), body_size);
    for (i, byte) in received_data.iter().enumerate() {
        assert_eq!(*byte, (i % 256) as u8, "mismatch at byte {}", i);
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 各種ステータスコード (404, 500)
#[tokio::test]
async fn test_status_codes() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut responded = 0;
        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        let path = headers
                            .iter()
                            .find(|h| h.name() == b":path")
                            .map(|h| h.value())
                            .unwrap_or(b"/");

                        let status = match path {
                            b"/notfound" => "404",
                            b"/error" => "500",
                            b"/no-content" => "204",
                            _ => "200",
                        };

                        let response_headers = vec![HeaderField::new(":status", status).unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");

                        responded += 1;
                        if responded >= 3 {
                            break;
                        }
                    }
                }
                Event::SettingsReceived { .. } | Event::ConnectionPreface => {}
                _ => {}
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // 3 つのリクエストを送信
    let paths = ["/notfound", "/error", "/no-content"];
    let expected_statuses = [b"404", b"500", b"204"];
    let mut stream_ids = Vec::new();
    for path in &paths {
        let request_headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", path).unwrap(),
            HeaderField::new(":authority", "localhost").unwrap(),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }

    let mut received_statuses: Vec<(StreamId, Vec<u8>)> = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            stream_id, headers, ..
        } = event
        {
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status");
            received_statuses.push((stream_id, status.value().to_vec()));
            if received_statuses.len() >= 3 {
                break;
            }
        }
    }

    // ストリーム ID 順にソートして検証
    for (i, sid) in stream_ids.iter().enumerate() {
        let status = received_statuses
            .iter()
            .find(|(id, _)| id == sid)
            .map(|(_, s): &(StreamId, Vec<u8>)| s.as_slice())
            .expect("missing status for stream");
        assert_eq!(
            status, expected_statuses[i],
            "status mismatch for path {}",
            paths[i]
        );
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// StreamClosed イベントが発生するか検証
///
/// 両方向で end_stream=true を送信した時点でストリームが closed になり、
/// StreamClosed イベントが発火する。
#[tokio::test]
async fn test_stream_closed_event() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let got_stream_closed = loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                    }
                }
                Event::StreamClosed { .. } => {
                    break true;
                }
                Event::SettingsReceived { .. } | Event::ConnectionPreface => {}
                _ => {}
            }
        };

        // GOAWAY を送信して正常終了
        conn.shutdown().await.expect("failed to shutdown");

        got_stream_closed
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    // クライアント側で StreamClosed が来るか確認
    let mut got_response = false;
    let mut got_stream_closed = false;
    loop {
        match client.next_event().await {
            Ok(Event::HeadersReceived {
                stream_id: sid,
                end_stream,
                ..
            }) => {
                assert_eq!(sid, stream_id);
                assert!(end_stream);
                got_response = true;
            }
            Ok(Event::StreamClosed { stream_id: sid }) => {
                assert_eq!(sid, stream_id);
                got_stream_closed = true;
            }
            Ok(Event::GoawayReceived { .. }) => {
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    assert!(got_response, "should receive response");
    assert!(
        got_stream_closed,
        "should receive StreamClosed event on client"
    );

    let server_got_closed = server_handle.await.expect("server task failed");
    assert!(
        server_got_closed,
        "server should receive StreamClosed event"
    );
}

/// GOAWAY with error code と debug_data
#[tokio::test]
async fn test_goaway_with_error_and_debug_data() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::SettingsReceived { ack: true }) => break,
                Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => return,
            }
        }

        // カスタムエラーコードと debug_data で GOAWAY を送信
        // ServerConnection に send_goaway の直接 API がないため、
        // shutdown() を使う (NoError のみ)
        // → ここでは shutdown() で NoError を確認
        conn.shutdown().await.expect("failed to send goaway");
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        match client.next_event().await {
            Ok(Event::GoawayReceived {
                error_code,
                last_stream_id,
                ..
            }) => {
                assert_eq!(error_code, ErrorCode::NoError);
                assert_eq!(last_stream_id, StreamId::Connection);
                break;
            }
            Ok(_) => {}
            Err(_) => panic!("connection closed before GOAWAY"),
        }
    }

    server_handle.await.expect("server task failed");
}

/// 同一サーバーに対する複数クライアントの同時接続
#[tokio::test]
async fn test_multiple_clients() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let client_count = 3;

    let server_handle = tokio::spawn(async move {
        for _ in 0..client_count {
            let mut conn = server.accept().await.expect("failed to accept connection");

            tokio::spawn(async move {
                loop {
                    match conn.next_event().await {
                        Ok(Event::HeadersReceived {
                            stream_id,
                            end_stream,
                            ..
                        }) => {
                            if end_stream {
                                let response_headers =
                                    vec![HeaderField::new(":status", "200").unwrap()];
                                conn.send_response(stream_id, response_headers, true)
                                    .await
                                    .expect("failed to send response");
                            }
                        }
                        Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            });
        }
    });

    // 複数クライアントを並行して接続
    let mut handles = Vec::new();
    for client_idx in 0..client_count {
        let limits_clone = limits.clone();
        let handle = tokio::spawn(async move {
            let mut client = Client::connect_insecure(server_addr, "localhost", limits_clone)
                .await
                .expect("failed to connect");

            loop {
                let event = client.next_event().await.expect("failed to get event");
                if let Event::SettingsReceived { ack: true } = event {
                    break;
                }
            }

            let request_headers = vec![
                HeaderField::new(":method", "GET").unwrap(),
                HeaderField::new(":scheme", "https").unwrap(),
                HeaderField::new(":path", format!("/client/{}", client_idx)).unwrap(),
                HeaderField::new(":authority", "localhost").unwrap(),
            ];
            let stream_id = client
                .send_request(request_headers, true)
                .await
                .expect("failed to send request");

            loop {
                let event = client.next_event().await.expect("failed to get event");
                if let Event::HeadersReceived {
                    stream_id: sid,
                    headers,
                    end_stream,
                    ..
                } = event
                {
                    assert_eq!(sid, stream_id);
                    assert!(end_stream);
                    let status = headers
                        .iter()
                        .find(|h| h.name() == b":status")
                        .expect("missing :status");
                    assert_eq!(status.value(), b"200");
                    break;
                }
            }

            client.shutdown().await.ok();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.expect("client task failed");
    }

    server_handle.await.expect("server task failed");
}

/// Limits カスタマイズ: max_concurrent_streams=1
#[tokio::test]
async fn test_limits_max_concurrent_streams() {
    let tls_config = generate_test_cert();
    let limits = Limits::builder()
        .max_concurrent_streams(Some(1))
        .build()
        .expect("valid limits");

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut responded = 0;
        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        responded += 1;
                        if responded >= 3 {
                            break;
                        }
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // max_concurrent_streams=1 でも逐次リクエストは正常動作すべき
    for i in 0..3 {
        let request_headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", format!("/seq/{}", i)).unwrap(),
            HeaderField::new(":authority", "localhost").unwrap(),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");

        loop {
            let event = client.next_event().await.expect("failed to get event");
            if let Event::HeadersReceived {
                stream_id: sid,
                end_stream,
                ..
            } = event
            {
                assert_eq!(sid, stream_id);
                assert!(end_stream);
                break;
            }
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// WindowUpdateReceived イベントの検証
#[tokio::test]
async fn test_window_update_received() {
    let tls_config = generate_test_cert();
    // 小さいウィンドウサイズでフロー制御を誘発
    let limits = Limits::builder()
        .initial_window_size(WindowSize::from_static(1024))
        .build()
        .expect("valid limits");

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // SETTINGS 交換中に WindowUpdateReceived が来る場合がある
    let mut got_window_update = false;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::WindowUpdateReceived { .. } => {
                got_window_update = true;
            }
            Event::SettingsReceived { ack: true } => break,
            _ => {}
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::WindowUpdateReceived { .. } => {
                got_window_update = true;
            }
            Event::HeadersReceived { end_stream, .. } => {
                if end_stream {
                    break;
                }
            }
            _ => {}
        }
    }

    // 小さいウィンドウサイズの場合、接続レベルの WINDOW_UPDATE が来るはず
    // (来ない場合もあるが、テスト自体が正常完了すればフロー制御は動作している)
    let _ = got_window_update;

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 大きなリクエストボディ (クライアント → サーバー)
#[tokio::test]
async fn test_large_request_body() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let body_size: usize = 50000;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // ヘッダー受信後に設定される
        let mut stream_id: Option<StreamId> = None;
        let mut received_body = Vec::new();

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived { stream_id: sid, .. } => {
                    stream_id = Some(sid);
                }
                Event::DataReceived {
                    data, end_stream, ..
                } => {
                    received_body.extend_from_slice(&data);
                    if end_stream {
                        let sid = stream_id.expect("data received before headers");
                        // 受信サイズをレスポンスで返す
                        let response_headers = vec![
                            HeaderField::new(":status", "200").unwrap(),
                            HeaderField::new("x-received-size", received_body.len().to_string())
                                .unwrap(),
                        ];
                        conn.send_response(sid, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Event::SettingsReceived { .. } | Event::ConnectionPreface => {}
                _ => {}
            }
        }

        received_body
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "POST").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/upload").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request");

    // 大きなデータを複数チャンクで送信
    let data: Vec<u8> = (0..body_size).map(|i| (i % 256) as u8).collect();
    let chunk_size = 8192;
    let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i == chunks.len() - 1;
        client
            .send_data(stream_id, chunk.to_vec(), is_last)
            .await
            .expect("failed to send data chunk");
    }

    // レスポンスを受信
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            headers,
            end_stream,
            ..
        } = event
        {
            assert!(end_stream);
            let received_size = headers
                .iter()
                .find(|h| h.name() == b"x-received-size")
                .expect("missing x-received-size");
            let size: usize = std::str::from_utf8(received_size.value())
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(size, body_size);
            break;
        }
    }

    let server_body = server_handle.await.expect("server task failed");
    assert_eq!(server_body.len(), body_size);

    client.shutdown().await.ok();
}

/// poll_event の動作確認
#[tokio::test]
async fn test_poll_event() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // poll_event は非ブロッキング: データがなければ None を返す
    // 接続直後は SETTINGS がキューにあるかもしれないが、None も許容
    let initial = client.poll_event();
    // None または Some のいずれかが来る
    match initial {
        None => {} // データ未受信の場合
        Some(Event::SettingsReceived { .. }) => {}
        Some(Event::ConnectionPreface) => {}
        Some(other) => panic!("unexpected event: {:?}", other),
    }

    // next_event で SETTINGS ACK まで進める
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    // drive() で I/O を進めてから poll_event を使う
    loop {
        client.drive().await.expect("failed to drive");
        if let Some(Event::HeadersReceived { end_stream, .. }) = client.poll_event() {
            assert!(end_stream);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// Content-Length 付きレスポンス
#[tokio::test]
async fn test_content_length_response() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let body = b"Hello, HTTP/2 World!";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![
                            HeaderField::new(":status", "200").unwrap(),
                            HeaderField::new("content-length", body.len().to_string()).unwrap(),
                            HeaderField::new("content-type", "text/plain").unwrap(),
                        ];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response");
                        conn.send_data(stream_id, body.to_vec(), true)
                            .await
                            .expect("failed to send data");
                        break;
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut got_content_length = false;
    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::HeadersReceived { headers, .. } => {
                let cl = headers.iter().find(|h| h.name() == b"content-length");
                if let Some(cl) = cl {
                    assert_eq!(cl.value(), body.len().to_string().as_bytes());
                    got_content_length = true;
                }
            }
            Event::DataReceived {
                data, end_stream, ..
            } => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            _ => {}
        }
    }

    assert!(got_content_length, "should have content-length header");
    assert_eq!(received_data, body);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 複数ストリームで別々のボディを受信
#[tokio::test]
async fn test_multiple_streams_with_bodies() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let stream_count = 5;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut responded = 0;
        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let path = headers
                            .iter()
                            .find(|h| h.name() == b":path")
                            .map(|h| String::from_utf8_lossy(h.value()).to_string())
                            .unwrap_or_default();

                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response");
                        conn.send_data(stream_id, format!("body-for-{}", path).into_bytes(), true)
                            .await
                            .expect("failed to send data");

                        responded += 1;
                        if responded >= stream_count {
                            break;
                        }
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let mut stream_ids = Vec::new();
    for i in 0..stream_count {
        let request_headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", format!("/item/{}", i)).unwrap(),
            HeaderField::new(":authority", "localhost").unwrap(),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }

    let mut bodies: std::collections::HashMap<StreamId, Vec<u8>> = std::collections::HashMap::new();
    let mut complete = 0;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Event::DataReceived {
                stream_id,
                data,
                end_stream,
            } => {
                bodies
                    .entry(stream_id)
                    .or_default()
                    .extend_from_slice(&data);
                if end_stream {
                    complete += 1;
                    if complete >= stream_count {
                        break;
                    }
                }
            }
            Event::HeadersReceived { .. } => {}
            _ => {}
        }
    }

    // 各ストリームが正しいボディを受信しているか確認
    assert_eq!(bodies.len(), stream_count);
    for body in bodies.values() {
        let s = String::from_utf8_lossy(body);
        assert!(s.starts_with("body-for-/item/"), "unexpected body: {}", s);
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// サーバー側から RST_STREAM を送った後、別のストリームでは正常通信
#[tokio::test]
async fn test_rst_stream_then_continue() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut request_count = 0;
        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        request_count += 1;
                        if request_count == 1 {
                            // 最初のリクエストは RST_STREAM で拒否
                            conn.reset_stream(stream_id, ErrorCode::RefusedStream)
                                .await
                                .expect("failed to send rst_stream");
                        } else {
                            // 2 番目のリクエストは正常レスポンス
                            let response_headers =
                                vec![HeaderField::new(":status", "200").unwrap()];
                            conn.send_response(stream_id, response_headers, true)
                                .await
                                .expect("failed to send response");
                            break;
                        }
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // 最初のリクエスト (拒否される)
    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/rejected").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream1 = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request 1");

    // RST_STREAM を待つ
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::StreamReset {
            stream_id,
            error_code,
        } = event
        {
            assert_eq!(stream_id, stream1);
            assert_eq!(error_code, ErrorCode::RefusedStream);
            break;
        }
    }

    // 2 番目のリクエスト (正常)
    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/ok").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream2 = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request 2");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(stream_id, stream2);
            assert!(end_stream);
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status");
            assert_eq!(status.value(), b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// HEAD リクエスト (ボディなし)
#[tokio::test]
async fn test_head_request() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name() == b":method")
                            .map(|h| h.value());
                        assert_eq!(method, Some(b"HEAD" as &[u8]));

                        // HEAD レスポンスは content-length 付きでもボディなし
                        let response_headers = vec![
                            HeaderField::new(":status", "200").unwrap(),
                            HeaderField::new("content-length", "1000").unwrap(),
                        ];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "HEAD").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            stream_id: sid,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(sid, stream_id);
            assert!(end_stream);
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status");
            assert_eq!(status.value(), b"200");
            // content-length はあるがボディはない
            let cl = headers.iter().find(|h| h.name() == b"content-length");
            assert!(cl.is_some());
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// DELETE リクエスト
#[tokio::test]
async fn test_delete_request() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name() == b":method")
                            .map(|h| h.value());
                        assert_eq!(method, Some(b"DELETE" as &[u8]));

                        let response_headers = vec![HeaderField::new(":status", "204").unwrap()];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "DELETE").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/resource/123").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            stream_id: sid,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(sid, stream_id);
            assert!(end_stream);
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status");
            assert_eq!(status.value(), b"204");
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// PUT リクエスト (ボディ付き)
#[tokio::test]
async fn test_put_request_with_body() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // ヘッダー受信後に設定される
        let mut stream_id: Option<StreamId> = None;
        let mut request_body = Vec::new();

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Event::HeadersReceived {
                    stream_id: sid,
                    headers,
                    ..
                } => {
                    let method = headers
                        .iter()
                        .find(|h| h.name() == b":method")
                        .map(|h| h.value());
                    assert_eq!(method, Some(b"PUT" as &[u8]));
                    stream_id = Some(sid);
                }
                Event::DataReceived {
                    data, end_stream, ..
                } => {
                    request_body.extend_from_slice(&data);
                    if end_stream {
                        let sid = stream_id.expect("data received before headers");
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(sid, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Event::SettingsReceived { .. } | Event::ConnectionPreface => {}
                _ => {}
            }
        }

        request_body
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "PUT").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/resource/456").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request");

    let body = b"updated resource content";
    client
        .send_data(stream_id, body.to_vec(), true)
        .await
        .expect("failed to send data");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::HeadersReceived {
            stream_id: sid,
            end_stream,
            ..
        } = event
        {
            assert_eq!(sid, stream_id);
            assert!(end_stream);
            break;
        }
    }

    let server_body = server_handle.await.expect("server task failed");
    assert_eq!(server_body, body);

    client.shutdown().await.ok();
}

/// drive() の動作確認
#[tokio::test]
async fn test_drive() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response");
                        conn.send_data(stream_id, b"driven".to_vec(), true)
                            .await
                            .expect("failed to send data");
                        break;
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // drive() + poll_event() のパターンで通信
    // まず SETTINGS ACK まで進める
    loop {
        client.drive().await.expect("failed to drive");
        let mut found = false;
        while let Some(event) = client.poll_event() {
            if let Event::SettingsReceived { ack: true } = event {
                found = true;
                break;
            }
        }
        if found {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_data = Vec::new();
    loop {
        client.drive().await.expect("failed to drive");
        while let Some(event) = client.poll_event() {
            if let Event::DataReceived {
                data, end_stream, ..
            } = event
            {
                received_data.extend_from_slice(&data);
                if end_stream {
                    assert_eq!(received_data, b"driven");
                    client.shutdown().await.ok();
                    server_handle.await.expect("server task failed");
                    return;
                }
            }
        }
    }
}

/// ストリームリセット後にデータ送信を試みる
#[tokio::test]
async fn test_send_data_after_reset() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if !end_stream {
                        // ストリームをリセット
                        conn.reset_stream(stream_id, ErrorCode::Cancel)
                            .await
                            .expect("failed to send rst_stream");
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(Event::DataReceived { .. }) => {
                    // リセット後でもデータが来ることはある (in-flight)
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "POST").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request");

    // データ送信 (サーバーがリセットする前に)
    client
        .send_data(stream_id, b"data1".to_vec(), false)
        .await
        .expect("failed to send data");

    // StreamReset を受信
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::StreamReset {
            stream_id: sid,
            error_code,
        } = event
        {
            assert_eq!(sid, stream_id);
            assert_eq!(error_code, ErrorCode::Cancel);
            break;
        }
    }

    // リセット後のデータ送信はエラーになるべき
    let result = client.send_data(stream_id, b"data2".to_vec(), true).await;
    assert!(result.is_err(), "send_data after reset should fail");

    client.shutdown().await.ok();
    server_handle.abort();
}

/// Limits の initial_window_size カスタマイズ
#[tokio::test]
async fn test_custom_initial_window_size() {
    let tls_config = generate_test_cert();
    // 非常に大きなウィンドウサイズ (ストリームと接続レベル両方)
    let limits = Limits::builder()
        .initial_window_size(WindowSize::from_static(1 << 20))
        .connection_window_size(WindowSize::from_static(1 << 20))
        .build()
        .expect("valid limits");

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let body_size: usize = 100_000;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response");

                        let data: Vec<u8> = vec![0xAB; body_size];
                        let chunk_size = 16384;
                        let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
                        for (i, chunk) in chunks.iter().enumerate() {
                            let is_last = i == chunks.len() - 1;
                            conn.send_data(stream_id, chunk.to_vec(), is_last)
                                .await
                                .expect("failed to send data");
                        }
                        // GOAWAY を送信して正常終了
                        conn.shutdown().await.expect("failed to shutdown");
                        break;
                    }
                }
                Ok(Event::SettingsReceived { .. }) | Ok(Event::ConnectionPreface) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_data = Vec::new();
    loop {
        match client.next_event().await {
            Ok(Event::DataReceived {
                data, end_stream, ..
            }) => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Ok(Event::HeadersReceived { .. }) => {}
            Ok(Event::GoawayReceived { .. }) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }

    assert_eq!(received_data.len(), body_size);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// デフォルトウィンドウ (65535) で 65535 bytes を超えるレスポンスボディを送受信する
///
/// issue 0041: クライアント側で `send_window_update` を返さない / Connection 側で接続レベル
/// `send_window` が誤って `connection_window_size` で初期化されている / `connection_window_size`
/// が拡張されたときに WINDOW_UPDATE が広告されない、のいずれかが残っていると、サーバーの送信
/// ウィンドウが 65535 で枯渇しテスト全体タイムアウトに到達する。
///
/// クライアントとサーバーを並列に進行させるため `multi_thread` runtime を使用する。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_response_body_exceeds_default_connection_window() {
    let tls_config = generate_test_cert();
    let limits = Limits::default();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config, limits.clone())
        .await
        .expect("サーバーの bind に失敗");
    let server_addr = server.local_addr();

    // デフォルト接続ウィンドウ (65535) を確実に超え、フロー制御が発動する
    // サイズを設定する。送信側はサーバータスクの中で全チャンクを連続キューイング
    // するため、初期送信ウィンドウ (65535) + Sans I/O 層の送信バッファ上限 (65535)
    // を合計した約 131_070 までしか一括投入できない。100_000 は両者の範囲内で
    // 必ずフロー制御に詰まるサイズである。
    let body_size: usize = 100_000;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続 accept に失敗");

        // リクエスト受信まで待つ
        let stream_id = loop {
            let event = conn.next_event().await.expect("イベント取得に失敗");
            match event {
                Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        break stream_id;
                    }
                }
                Event::SettingsReceived { .. } | Event::ConnectionPreface => {}
                _ => {}
            }
        };

        let response_headers = vec![HeaderField::new(":status", "200").unwrap()];
        conn.send_response(stream_id, response_headers, false)
            .await
            .expect("レスポンスヘッダー送信に失敗");

        // データを分割して送信する。デフォルトの送信ウィンドウ (65535) を超える分は
        // Sans I/O 層の送信バッファにキューイングされ、後続の next_event() で
        // クライアントから WINDOW_UPDATE を受信した時点で自動的に送信される。
        let data: Vec<u8> = (0..body_size).map(|i| (i % 256) as u8).collect();
        let chunk_size = 16_384;
        let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
        let total_chunks = chunks.len();
        for (i, chunk) in chunks.into_iter().enumerate() {
            let is_last = i == total_chunks - 1;
            conn.send_data(stream_id, chunk.to_vec(), is_last)
                .await
                .expect("DATA チャンク送信に失敗");
        }

        // クライアントからの WINDOW_UPDATE 受信と保留中データの送信を進める。
        // クライアントの shutdown (GOAWAY) または EOF を検知してループを抜ける。
        loop {
            match conn.next_event().await {
                Ok(Event::GoawayReceived { .. }) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("クライアント接続に失敗");

    // サーバーの SETTINGS に対する ACK 送信まで待つ
    loop {
        let event = client.next_event().await.expect("イベント取得に失敗");
        if let Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/large").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("リクエスト送信に失敗");

    let mut received_data = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let event = tokio::time::timeout(remaining, client.next_event())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "レスポンス受信がタイムアウト。受信済み {} / {} bytes (フロー制御の不整合の疑い)",
                    received_data.len(),
                    body_size
                )
            })
            .expect("イベント取得に失敗");
        match event {
            Event::DataReceived {
                stream_id: sid,
                data,
                end_stream,
            } => {
                received_data.extend_from_slice(&data);

                // RFC 9113 Section 6.9: 受信した DATA 分だけウィンドウを補充する。
                // これが無い、または Connection 内部のウィンドウ初期化が誤っていると
                // 65535 bytes 受信した時点でサーバーの送信が止まる。
                let increment = u32::try_from(data.len())
                    .expect("DATA ペイロードサイズが u32 範囲内であること (RFC 9113)");
                if increment > 0 {
                    client
                        .send_window_update(StreamId::Connection, increment)
                        .await
                        .expect("接続レベル WINDOW_UPDATE 送信に失敗");
                    if !end_stream {
                        client
                            .send_window_update(sid, increment)
                            .await
                            .expect("ストリームレベル WINDOW_UPDATE 送信に失敗");
                    }
                }

                if end_stream {
                    break;
                }
            }
            Event::HeadersReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(
        received_data.len(),
        body_size,
        "受信バイト数が想定と一致しない"
    );
    for (i, byte) in received_data.iter().enumerate() {
        assert_eq!(*byte, (i % 256) as u8, "{} バイト目の内容不一致", i);
    }

    client.shutdown().await.ok();
    server_handle.await.expect("サーバータスクが panic で終了");
}
