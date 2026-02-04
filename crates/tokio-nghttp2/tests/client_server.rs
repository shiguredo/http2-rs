//! クライアント/サーバー統合テスト

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_nghttp2::{Client, ErrorCode, Header, Http2Event, Server, TlsServerConfig};

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

    // サーバー起動
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config)
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
                Http2Event::SettingsReceived { ack: false } => {
                    // 初期 SETTINGS 受信
                }
                Http2Event::SettingsReceived { ack: true } => {
                    // SETTINGS ACK 受信
                }
                Http2Event::HeadersReceived {
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
                    let response_headers = vec![Header::status(200)];
                    conn.send_response(stream_id, &response_headers, true)
                        .await
                        .expect("failed to send response");
                    conn.flush().await.expect("failed to flush");

                    break;
                }
                _ => {}
            }
        }
    });

    // クライアント接続
    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // リクエスト送信
    let request_headers = vec![
        Header::method("GET"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    // レスポンス待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::SettingsReceived { .. } => {
                // SETTINGS 処理
            }
            Http2Event::HeadersReceived {
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

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config)
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // イベントループ (PING に対する PONG は nghttp2 が自動で送信する)
        loop {
            match conn.next_event().await {
                Ok(Http2Event::PingReceived { ack, .. }) => {
                    if !ack {
                        // PING 受信 (nghttp2 が自動で PONG を送信)
                        conn.flush().await.expect("failed to flush");
                    }
                }
                Ok(Http2Event::SettingsReceived { .. }) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // 初期 SETTINGS を処理
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // PING 送信
    let ping_data = [1, 2, 3, 4, 5, 6, 7, 8];
    client.ping(&ping_data).await.expect("failed to send ping");
    client.flush().await.expect("failed to flush");

    // PONG 待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::PingReceived { opaque_data, ack } = event {
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

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config)
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut received_settings = false;
        let mut received_ack = false;

        loop {
            match conn.next_event().await {
                Ok(Http2Event::SettingsReceived { ack }) => {
                    if ack {
                        received_ack = true;
                    } else {
                        received_settings = true;
                    }
                    if received_settings && received_ack {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(received_settings);
        assert!(received_ack);
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    let mut received_settings = false;
    let mut received_ack = false;

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack } = event {
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

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config)
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut streams_received = 0;

        loop {
            match conn.next_event().await {
                Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        // レスポンス送信
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");

                        streams_received += 1;
                        if streams_received >= 3 {
                            break;
                        }
                    }
                }
                Ok(Http2Event::SettingsReceived { .. }) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // 初期 SETTINGS を処理
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // 3 つのリクエストを送信
    let mut stream_ids = Vec::new();
    for i in 0..3 {
        let request_headers = vec![
            Header::method("GET"),
            Header::scheme("https"),
            Header::authority("localhost"),
            Header::path(format!("/path{}", i).as_str()),
        ];
        let stream_id = client
            .send_request(&request_headers, None, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }
    client.flush().await.expect("failed to flush");

    // 3 つのレスポンスを受信
    let mut responses_received = 0;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::HeadersReceived {
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

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config)
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // 初期 SETTINGS を処理
        loop {
            match conn.next_event().await {
                Ok(Http2Event::SettingsReceived { ack: true }) => break,
                Ok(_) => {}
                Err(_) => return,
            }
        }

        // GOAWAY 送信
        conn.shutdown(0).await.expect("failed to send goaway");
        conn.flush().await.expect("failed to flush");
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // GOAWAY 待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::GoawayReceived {
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

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config)
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        // RST_STREAM を送信
                        conn.reset_stream(stream_id, ErrorCode::Cancel)
                            .await
                            .expect("failed to send rst_stream");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Http2Event::SettingsReceived { .. }) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // 初期 SETTINGS を処理
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // リクエスト送信
    let request_headers = vec![
        Header::method("GET"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    // StreamClosed イベント待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::StreamClosed {
            stream_id: closed_stream_id,
            error_code,
        } = event
        {
            assert_eq!(closed_stream_id, stream_id);
            assert_eq!(error_code, ErrorCode::Cancel);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// レスポンスボディ付きレスポンス
#[tokio::test]
async fn test_response_with_body() {
    let tls_config = generate_test_cert();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config)
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let response_body: &[u8] = b"Hello, World!";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        // レスポンスヘッダー送信 (end_stream=false でボディを後で送信)
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");

                        // レスポンスボディ送信
                        conn.send_data(stream_id, b"Hello, World!", true)
                            .await
                            .expect("failed to send data");

                        break;
                    }
                }
                Http2Event::SettingsReceived { .. } => {}
                _ => {}
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // 初期 SETTINGS を処理
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // リクエスト送信
    let request_headers = vec![
        Header::method("GET"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");

    // レスポンスヘッダーとボディを受信
    let mut got_headers = false;
    let mut got_data = false;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(!end_stream);
                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value, b"200");
                got_headers = true;
            }
            Http2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert_eq!(data, response_body);
                assert!(end_stream);
                got_data = true;
            }
            Http2Event::SettingsReceived { .. } => {}
            _ => {}
        }
        if got_headers && got_data {
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// リクエストボディ付きリクエスト
#[tokio::test]
async fn test_request_with_body() {
    let tls_config = generate_test_cert();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls_config)
        .await
        .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut got_headers = false;
        let mut got_data = false;
        let mut recv_stream_id = 0;

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                } => {
                    assert!(!end_stream);
                    let method = headers
                        .iter()
                        .find(|h| h.name == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value, b"POST");
                    recv_stream_id = stream_id;
                    got_headers = true;
                }
                Http2Event::DataReceived {
                    stream_id,
                    data,
                    end_stream,
                } => {
                    assert_eq!(stream_id, recv_stream_id);
                    assert_eq!(data, b"request body data");
                    assert!(end_stream);
                    got_data = true;

                    // レスポンス送信
                    let response_headers = vec![Header::status(200)];
                    conn.send_response(stream_id, &response_headers, true)
                        .await
                        .expect("failed to send response");
                }
                Http2Event::SettingsReceived { .. } => {}
                _ => {}
            }
            if got_headers && got_data {
                break;
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // 初期 SETTINGS を処理
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    // POST リクエスト送信 (ボディ付き)
    let request_headers = vec![
        Header::method("POST"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, Some(b"request body data"), true)
        .await
        .expect("failed to send request");

    // レスポンス待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(end_stream);
                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value, b"200");
                break;
            }
            Http2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}
