//! クライアント/サーバー統合テスト

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_http2::{Client, ErrorCode, Event, HeaderField, Limits, Server, TlsServerConfig};

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
                } => {
                    // リクエストヘッダー受信
                    assert!(end_stream);

                    // :method ヘッダーを確認
                    let method = headers
                        .iter()
                        .find(|h| h.name == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value, b"GET");

                    // :path ヘッダーを確認
                    let path = headers
                        .iter()
                        .find(|h| h.name == b":path")
                        .expect("missing :path header");
                    assert_eq!(path.value, b"/");

                    // レスポンス送信
                    let response_headers = vec![HeaderField::from_str(":status", "200")];
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
        HeaderField::from_str(":method", "GET"),
        HeaderField::from_str(":scheme", "https"),
        HeaderField::from_str(":path", "/"),
        HeaderField::from_str(":authority", "localhost"),
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
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(end_stream);

                // :status ヘッダーを確認
                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value, b"200");

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
        match event {
            Event::SettingsReceived { ack } => {
                if ack {
                    received_ack = true;
                } else {
                    received_settings = true;
                }
                if received_settings && received_ack {
                    break;
                }
            }
            _ => {}
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
                        let response_headers = vec![HeaderField::from_str(":status", "200")];
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", &format!("/path{}", i)),
            HeaderField::from_str(":authority", "localhost"),
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
        } = event
        {
            assert!(stream_ids.contains(&stream_id));
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");

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
            assert_eq!(last_stream_id, 0);
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
        HeaderField::from_str(":method", "GET"),
        HeaderField::from_str(":scheme", "https"),
        HeaderField::from_str(":path", "/"),
        HeaderField::from_str(":authority", "localhost"),
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
