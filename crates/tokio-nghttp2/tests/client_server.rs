//! クライアント/サーバー統合テスト
#![allow(clippy::collapsible_match)]

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_nghttp2::{
    Client, ErrorCode, Header, Http2Event, Server, SessionOptions, SettingsId, TlsServerConfig,
};

/// テスト用自己署名証明書を生成
fn generate_test_cert() -> TlsServerConfig {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("failed to generate certificate");

    TlsServerConfig::from_der(
        vec![CertificateDer::from(cert.der().to_vec())],
        PrivateKeyDer::try_from(signing_key.serialize_der()).expect("should succeed"),
    )
    .expect("failed to create TLS server config")
}

/// 基本的なリクエスト/レスポンス
#[tokio::test]
async fn test_basic_request_response() {
    let tls_config = generate_test_cert();

    // サーバー起動
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

/// 大量データ転送 (フロー制御が動作することを確認)
#[tokio::test]
async fn test_large_data_transfer() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    // 100KB のデータを生成
    let large_data: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
    let expected_data = large_data.clone();

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
                        // レスポンスヘッダー送信
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");

                        // 大量データ送信
                        conn.send_data(stream_id, &large_data, true)
                            .await
                            .expect("failed to send data");

                        conn.flush().await.expect("failed to flush");
                    }
                }
                Http2Event::StreamClosed { .. } => {
                    // ストリームが閉じたら終了
                    break;
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
        Header::path("/large"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");

    // レスポンスを受信 (複数の DATA フレームに分割される可能性がある)
    let mut received_data = Vec::new();
    let mut got_headers = false;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(!end_stream);
                got_headers = true;
            }
            Http2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::SettingsReceived { .. } => {}
            Http2Event::WindowUpdateReceived { .. } => {
                // フロー制御により WINDOW_UPDATE が発生する可能性がある
            }
            _ => {}
        }
    }

    assert!(got_headers);
    assert_eq!(received_data, expected_data);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 複数 DATA フレームを分割送信
#[tokio::test]
async fn test_multiple_data_frames() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

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
                        // レスポンスヘッダー送信
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");

                        // 複数の DATA フレームを送信
                        conn.send_data(stream_id, b"Part1-", false)
                            .await
                            .expect("failed to send data 1");
                        conn.send_data(stream_id, b"Part2-", false)
                            .await
                            .expect("failed to send data 2");
                        conn.send_data(stream_id, b"Part3", true)
                            .await
                            .expect("failed to send data 3");

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

    // 複数の DATA フレームを受信
    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::HeadersReceived { .. } => {}
            Http2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_data, b"Part1-Part2-Part3");

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// クライアント側からのシャットダウン (GOAWAY 送信)
#[tokio::test]
async fn test_client_shutdown() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

        // クライアントからの GOAWAY を待機
        loop {
            match conn.next_event().await {
                Ok(Http2Event::GoawayReceived {
                    last_stream_id,
                    error_code,
                    ..
                }) => {
                    assert_eq!(last_stream_id, 0);
                    assert_eq!(error_code, ErrorCode::NoError);
                    break;
                }
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

    // クライアント側から shutdown (GOAWAY 送信)
    client.shutdown().await.expect("failed to shutdown");

    server_handle.await.expect("server task failed");
}

/// 異なるエラーコードでの RST_STREAM
#[tokio::test]
async fn test_rst_stream_internal_error() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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
                        // InternalError で RST_STREAM を送信
                        conn.reset_stream(stream_id, ErrorCode::InternalError)
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

    // StreamClosed イベント待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::StreamClosed {
            stream_id: closed_stream_id,
            error_code,
        } = event
        {
            assert_eq!(closed_stream_id, stream_id);
            assert_eq!(error_code, ErrorCode::InternalError);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// HEAD リクエスト (ボディなしレスポンス)
#[tokio::test]
async fn test_head_request() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                } => {
                    // HEAD メソッドを確認
                    let method = headers
                        .iter()
                        .find(|h| h.name == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value, b"HEAD");

                    if end_stream {
                        // HEAD レスポンス (ボディなし)
                        let response_headers =
                            vec![Header::status(200), Header::new("content-length", "1000")];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
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

    // HEAD リクエスト送信
    let request_headers = vec![
        Header::method("HEAD"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
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

                let content_length = headers
                    .iter()
                    .find(|h| h.name == b"content-length")
                    .expect("missing content-length header");
                assert_eq!(content_length.value, b"1000");

                break;
            }
            Http2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// カスタムヘッダー
#[tokio::test]
async fn test_custom_headers() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                } => {
                    // カスタムヘッダーを確認
                    let custom_header = headers
                        .iter()
                        .find(|h| h.name == b"x-custom-header")
                        .expect("missing x-custom-header");
                    assert_eq!(custom_header.value, b"custom-value");

                    if end_stream {
                        // カスタムヘッダー付きレスポンス
                        let response_headers = vec![
                            Header::status(200),
                            Header::new("x-response-header", "response-value"),
                        ];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
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

    // カスタムヘッダー付きリクエスト送信
    let request_headers = vec![
        Header::method("GET"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
        Header::new("x-custom-header", "custom-value"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
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

                let response_header = headers
                    .iter()
                    .find(|h| h.name == b"x-response-header")
                    .expect("missing x-response-header");
                assert_eq!(response_header.value, b"response-value");

                break;
            }
            Http2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 空のリクエストボディ (Content-Length: 0)
#[tokio::test]
async fn test_empty_request_body() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                } => {
                    let method = headers
                        .iter()
                        .find(|h| h.name == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value, b"POST");

                    // POST で end_stream=true は空ボディ
                    if end_stream {
                        let response_headers = vec![Header::status(204)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
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

    // 空ボディの POST リクエスト
    let request_headers = vec![
        Header::method("POST"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true) // end_stream=true で空ボディ
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
                assert_eq!(status.value, b"204");

                break;
            }
            Http2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// トレーラー送受信
#[tokio::test]
async fn test_trailer() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

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
                        // レスポンスヘッダー送信 (end_stream=false)
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");

                        // トレーラー用データ送信
                        conn.send_data_for_trailer(stream_id, b"response body")
                            .await
                            .expect("failed to send data for trailer");

                        // トレーラー送信
                        let trailer = vec![Header::new("x-checksum", "abc123")];
                        conn.send_trailer(stream_id, &trailer)
                            .await
                            .expect("failed to send trailer");

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

    // レスポンスヘッダー、データ、トレーラーを受信
    let mut got_headers = false;
    let mut got_data = false;
    let mut got_trailer = false;

    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                if !got_headers {
                    // レスポンスヘッダー
                    assert!(!end_stream);
                    let status = headers
                        .iter()
                        .find(|h| h.name == b":status")
                        .expect("missing :status header");
                    assert_eq!(status.value, b"200");
                    got_headers = true;
                } else {
                    // トレーラー
                    assert!(end_stream);
                    let checksum = headers
                        .iter()
                        .find(|h| h.name == b"x-checksum")
                        .expect("missing x-checksum trailer");
                    assert_eq!(checksum.value, b"abc123");
                    got_trailer = true;
                }
            }
            Http2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert_eq!(data, b"response body");
                assert!(!end_stream);
                got_data = true;
            }
            Http2Event::SettingsReceived { .. } => {}
            _ => {}
        }
        if got_headers && got_data && got_trailer {
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// graceful shutdown
#[tokio::test]
async fn test_graceful_shutdown() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

        // graceful shutdown 通知 (GOAWAY with last_stream_id=2^31-1)
        conn.shutdown_graceful()
            .await
            .expect("failed to send shutdown notice");

        // クライアントがイベントを受信する時間を確保
        conn.flush().await.expect("failed to flush");

        // 最終 GOAWAY を送信
        conn.shutdown(0).await.expect("failed to send final goaway");
        conn.flush().await.expect("failed to flush");
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // GOAWAY を待機
    let mut received_goaway = false;
    let mut received_final_goaway = false;
    loop {
        match client.next_event().await {
            Ok(Http2Event::GoawayReceived { last_stream_id, .. }) => {
                if last_stream_id == 0 {
                    received_final_goaway = true;
                } else {
                    received_goaway = true;
                }
                if received_final_goaway {
                    break;
                }
            }
            Ok(Http2Event::SettingsReceived { .. }) => {}
            Ok(_) => {}
            Err(_) => break,
        }
    }

    // graceful shutdown 通知または最終 GOAWAY を受信していること
    assert!(received_goaway || received_final_goaway);

    server_handle.await.expect("server task failed");
}

/// SessionOptions 付き接続
#[tokio::test]
async fn test_session_options() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let server_options = SessionOptions::new()
            .expect("failed to create session options")
            .peer_max_concurrent_streams(50);
        let mut conn = server
            .accept_with_options(&server_options)
            .await
            .expect("failed to accept connection");

        // リクエスト待機
        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Http2Event::SettingsReceived { .. } => {}
                _ => {}
            }
        }
    });

    let client_options = SessionOptions::new()
        .expect("failed to create session options")
        .peer_max_concurrent_streams(100);

    let tls_client_config =
        tokio_nghttp2::TlsClientConfig::insecure().expect("failed to create insecure TLS config");

    let mut client =
        Client::connect_with_options(server_addr, "localhost", tls_client_config, &client_options)
            .await
            .expect("failed to connect with options");

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

    // レスポンス待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(end_stream);
                break;
            }
            Http2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// SETTINGS 取得
#[tokio::test]
async fn test_settings_query() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        // SETTINGS 交換完了まで待機
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

        // ローカル設定を確認
        let local_initial_window_size = conn.get_local_settings(SettingsId::InitialWindowSize);
        assert!(local_initial_window_size > 0);

        // リモート設定を確認
        let remote_initial_window_size = conn.get_remote_settings(SettingsId::InitialWindowSize);
        assert!(remote_initial_window_size > 0);

        // リクエストを処理して終了
        loop {
            match conn.next_event().await {
                Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // SETTINGS 交換完了まで待機
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

    // クライアント側の設定を確認
    let local_initial_window_size = client.get_local_settings(SettingsId::InitialWindowSize);
    assert!(local_initial_window_size > 0);

    let remote_initial_window_size = client.get_remote_settings(SettingsId::InitialWindowSize);
    assert!(remote_initial_window_size > 0);

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

    // レスポンス待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// terminate_session
#[tokio::test]
async fn test_terminate_session() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
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

        // terminate_session で InternalError を送信
        conn.terminate(ErrorCode::InternalError)
            .await
            .expect("failed to terminate session");
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    // GOAWAY 待機
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::GoawayReceived { error_code, .. } = event {
            assert_eq!(error_code, ErrorCode::InternalError);
            break;
        }
    }

    server_handle.await.expect("server task failed");
}

// ============================================================================
// 嫌がらせ系テスト
// ============================================================================

/// 大量ストリームを同時に開く
#[tokio::test]
async fn test_many_concurrent_streams() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let stream_count = 50;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut streams_responded = 0;
        loop {
            match conn.next_event().await {
                Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");

                        streams_responded += 1;
                        if streams_responded >= stream_count {
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

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let mut stream_ids = Vec::new();
    for i in 0..stream_count {
        let request_headers = vec![
            Header::method("GET"),
            Header::scheme("https"),
            Header::authority("localhost"),
            Header::path(format!("/path/{}", i).as_str()),
        ];
        let stream_id = client
            .send_request(&request_headers, None, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }

    let mut responses = 0;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::HeadersReceived {
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let rst_count = 10;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut rst_sent = 0;
        loop {
            match conn.next_event().await {
                Ok(Http2Event::HeadersReceived {
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
                Ok(Http2Event::SettingsReceived { .. }) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let mut stream_ids = Vec::new();
    for _ in 0..rst_count {
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
        stream_ids.push(stream_id);
    }

    let mut closed = 0;
    loop {
        match client.next_event().await {
            Ok(Http2Event::StreamClosed { .. }) => {
                closed += 1;
                if closed >= rst_count {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert_eq!(closed, rst_count);

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 連続 PING 送信
#[tokio::test]
async fn test_rapid_ping() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let ping_count: u8 = 20;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match conn.next_event().await {
                Ok(Http2Event::PingReceived { ack, .. }) => {
                    if !ack {
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

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    for i in 0..ping_count {
        let data = [i, 0, 0, 0, 0, 0, 0, 0];
        client.ping(&data).await.expect("failed to send ping");
    }

    let mut pong_count: u8 = 0;
    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::PingReceived { ack: true, .. } = event {
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let chunk_count: usize = 100;

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
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");

                        for i in 0..chunk_count {
                            let is_last = i == chunk_count - 1;
                            conn.send_data(stream_id, &[i as u8], is_last)
                                .await
                                .expect("failed to send data");
                        }
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

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        Header::method("GET"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");

    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::HeadersReceived { .. } => {}
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let header_count = 50;

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                } => {
                    if end_stream {
                        let custom_count = headers
                            .iter()
                            .filter(|h| h.name.starts_with(b"x-test-"))
                            .count();
                        assert_eq!(custom_count, header_count);

                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
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

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let mut request_headers = vec![
        Header::method("GET"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    for i in 0..header_count {
        request_headers.push(Header::new(
            format!("x-test-{}", i).as_str(),
            format!("value-{}", i).as_str(),
        ));
    }

    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::HeadersReceived {
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let stream_id = loop {
            match conn.next_event().await {
                Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        break stream_id;
                    }
                }
                Ok(Http2Event::SettingsReceived { .. }) => {}
                Ok(_) => {}
                Err(_) => return,
            }
        };

        // GOAWAY を送信 (既存ストリームは処理する)
        conn.shutdown(stream_id)
            .await
            .expect("failed to send goaway");

        // 既存ストリームにはレスポンスを返す
        let response_headers = vec![Header::status(200)];
        conn.send_response(stream_id, &response_headers, true)
            .await
            .expect("failed to send response");
        conn.flush().await.expect("failed to flush");
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

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

    let mut got_goaway = false;
    let mut got_response = false;
    loop {
        match client.next_event().await {
            Ok(Http2Event::GoawayReceived { .. }) => got_goaway = true,
            Ok(Http2Event::HeadersReceived {
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

/// 空のデータフレーム
#[tokio::test]
async fn test_empty_data_frames() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

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
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");

                        conn.send_data(stream_id, b"", false)
                            .await
                            .expect("failed to send empty data");
                        conn.send_data(stream_id, b"content", false)
                            .await
                            .expect("failed to send data");
                        conn.send_data(stream_id, b"", true)
                            .await
                            .expect("failed to send final empty data");
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

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        Header::method("GET"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/"),
    ];
    client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");

    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::HeadersReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_data, b"content");

    client.shutdown().await.ok();
    server_handle.await.expect("server task failed");
}

/// 双方向ストリーミング (エコーサーバー)
#[tokio::test]
async fn test_bidirectional_streaming() {
    let tls_config = generate_test_cert();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut stream_id = 0;

        loop {
            let event = conn.next_event().await.expect("failed to get event");
            match event {
                Http2Event::HeadersReceived { stream_id: sid, .. } => {
                    stream_id = sid;
                    let response_headers = vec![Header::status(200)];
                    conn.send_response(stream_id, &response_headers, false)
                        .await
                        .expect("failed to send response");
                }
                Http2Event::DataReceived {
                    data, end_stream, ..
                } => {
                    conn.send_data(stream_id, &data, end_stream)
                        .await
                        .expect("failed to echo data");
                    if end_stream {
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

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    let request_headers = vec![
        Header::method("POST"),
        Header::scheme("https"),
        Header::authority("localhost"),
        Header::path("/echo"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, false)
        .await
        .expect("failed to send request");

    client
        .send_data(stream_id, b"chunk1-", false)
        .await
        .expect("failed to send data 1");
    client
        .send_data(stream_id, b"chunk2-", false)
        .await
        .expect("failed to send data 2");
    client
        .send_data(stream_id, b"chunk3", true)
        .await
        .expect("failed to send data 3");

    let mut received_data = Vec::new();
    loop {
        let event = client.next_event().await.expect("failed to get event");
        match event {
            Http2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_data.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::HeadersReceived { .. } => {}
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

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut responded = 0;

        loop {
            match conn.next_event().await {
                Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                }) => {
                    if end_stream {
                        let response_headers = vec![Header::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");
                        conn.send_data(
                            stream_id,
                            format!("response-{}", responded).as_bytes(),
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
                Ok(Http2Event::SettingsReceived { .. }) => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut client = Client::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    loop {
        let event = client.next_event().await.expect("failed to get event");
        if let Http2Event::SettingsReceived { ack: true } = event {
            break;
        }
    }

    for _ in 0..5 {
        let request_headers = vec![
            Header::method("GET"),
            Header::scheme("https"),
            Header::authority("localhost"),
            Header::path("/"),
        ];
        client
            .send_request(&request_headers, None, true)
            .await
            .expect("failed to send request");
    }

    let mut complete = 0;
    loop {
        match client.next_event().await {
            Ok(Http2Event::DataReceived { end_stream, .. }) => {
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
