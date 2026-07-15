//! tokio-nghttp2 と tokio-http2 の相互運用テスト
#![allow(clippy::collapsible_match)]

use std::time::Duration;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use shiguredo_nghttp2::SettingsId;
use tokio_http2::{
    ErrorCode as Http2ErrorCode, Event as Http2Event, HeaderField, Limits, Server as Http2Server,
    TlsServerConfig as Http2TlsServerConfig,
};
use tokio_nghttp2::{
    Client as NgClient, ErrorCode as NgErrorCode, Header as NgHeader, Http2Event as NgHttp2Event,
    Server as NgServer, TlsServerConfig as NgTlsServerConfig,
};

/// tokio-http2 用テスト証明書を生成
fn generate_http2_test_cert() -> Http2TlsServerConfig {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("failed to generate certificate");

    Http2TlsServerConfig::from_der(
        vec![CertificateDer::from(cert.der().to_vec())],
        PrivateKeyDer::try_from(signing_key.serialize_der()).expect("should succeed"),
    )
    .expect("failed to create TLS server config")
}

/// tokio-nghttp2 用テスト証明書を生成
fn generate_nghttp2_test_cert() -> NgTlsServerConfig {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("failed to generate certificate");

    NgTlsServerConfig::from_der(
        vec![CertificateDer::from(cert.der().to_vec())],
        PrivateKeyDer::try_from(signing_key.serialize_der()).expect("should succeed"),
    )
    .expect("failed to create TLS server config")
}

/// tokio-http2 クライアントが SETTINGS ACK を受信するまで待機
async fn wait_for_http2_settings_ack(client: &mut tokio_http2::Client) {
    loop {
        match tokio::time::timeout(Duration::from_secs(2), client.next_event()).await {
            Ok(Ok(Http2Event::SettingsReceived { ack: true })) => break,
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
}

/// nghttp2 クライアントが SETTINGS ACK を受信するまで待機
async fn wait_for_nghttp2_settings_ack(client: &mut NgClient) {
    loop {
        match tokio::time::timeout(Duration::from_secs(2), client.next_event()).await {
            Ok(Ok(NgHttp2Event::SettingsReceived { ack: true })) => break,
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
}

// ============================================================================
// 基本通信テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 基本リクエスト/レスポンス
#[tokio::test]
async fn test_nghttp2_client_http2_server_basic() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await
            {
                Ok(Ok(e)) => e,
                _ => break,
            };
            match event {
                Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name() == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value(), b"GET");

                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");

                        // クライアントがレスポンスを受信するまで待機
                        let _ =
                            tokio::time::timeout(Duration::from_secs(2), conn.next_event()).await;
                        break;
                    }
                }
                Http2Event::SettingsReceived { .. }
                | Http2Event::ConnectionPreface
                | Http2Event::WindowUpdateReceived { .. } => {}
                _ => {}
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
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
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 基本リクエスト/レスポンス
#[tokio::test]
async fn test_http2_client_nghttp2_server_basic() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            let event = match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await
            {
                Ok(Ok(e)) => e,
                _ => break,
            };
            match event {
                NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                } => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value, b"GET");

                        let response_headers = vec![NgHeader::status(200)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");

                        // クライアントがレスポンスを受信するまで待機
                        let _ =
                            tokio::time::timeout(Duration::from_secs(2), conn.next_event()).await;
                        break;
                    }
                }
                NgHttp2Event::SettingsReceived { .. } => {}
                _ => {}
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    // nghttp2 との相互運用では SETTINGS ACK を待つ必要がある
    wait_for_http2_settings_ack(&mut client).await;

    // RFC 9113 Section 8.3.1: :method, :scheme, :path, :authority が必須
    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value(), b"200");
                break;
            }
            Http2Event::SettingsReceived { .. } | Http2Event::ConnectionPreface => {}
            _ => {}
        }
    }

    let _ = server_handle.await;
}

// ============================================================================
// 複数ストリームテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 複数ストリーム
#[tokio::test]
async fn test_nghttp2_client_http2_server_multiple_streams() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut streams_received = 0;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");

                        streams_received += 1;
                        if streams_received >= 3 {
                            break;
                        }
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let mut stream_ids = Vec::new();
    for i in 0..3 {
        let request_headers = vec![
            NgHeader::method("GET"),
            NgHeader::scheme("https"),
            NgHeader::authority("localhost"),
            NgHeader::path(format!("/path{}", i).as_str()),
        ];
        let stream_id = client
            .send_request(&request_headers, None, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }
    client.flush().await.expect("failed to flush");

    let mut responses_received = 0;
    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::HeadersReceived {
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
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 複数ストリーム
#[tokio::test]
async fn test_http2_client_nghttp2_server_multiple_streams() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
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
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![NgHeader::status(200)];
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
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let mut stream_ids = Vec::new();
    for i in 0..3 {
        let request_headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", format!("/path{}", i)).expect("valid header field"),
            HeaderField::new(":authority", "localhost").expect("valid header field"),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");
        stream_ids.push(stream_id);
    }

    let mut responses_received = 0;
    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::HeadersReceived {
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
    let _ = server_handle.await;
}

// ============================================================================
// RST_STREAM テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: RST_STREAM
#[tokio::test]
async fn test_nghttp2_client_http2_server_rst_stream() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        conn.reset_stream(stream_id, Http2ErrorCode::Cancel)
                            .await
                            .expect("failed to send rst_stream");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::StreamClosed {
            stream_id: closed_stream_id,
            error_code,
        } = event
        {
            assert_eq!(closed_stream_id, stream_id);
            assert_eq!(error_code, NgErrorCode::Cancel);
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: RST_STREAM
#[tokio::test]
async fn test_http2_client_nghttp2_server_rst_stream() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        conn.reset_stream(stream_id, NgErrorCode::Cancel)
                            .await
                            .expect("failed to send rst_stream");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::StreamReset {
            stream_id: reset_stream_id,
            error_code,
        } = event
        {
            assert_eq!(reset_stream_id, stream_id);
            assert_eq!(error_code, Http2ErrorCode::Cancel);
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// SETTINGS テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: SETTINGS 交換
#[tokio::test]
async fn test_nghttp2_client_http2_server_settings() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        let mut received_settings = false;
        let mut received_ack = false;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::SettingsReceived { ack })) => {
                    if ack {
                        received_ack = true;
                    } else {
                        received_settings = true;
                    }
                    if received_settings && received_ack {
                        break;
                    }
                }
                Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(received_settings);
        assert!(received_ack);

        conn.shutdown().await.expect("failed to send goaway");
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    let mut received_settings = false;
    let mut received_ack = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::SettingsReceived { ack } = event {
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
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: SETTINGS 交換
#[tokio::test]
async fn test_http2_client_nghttp2_server_settings() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
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
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::SettingsReceived { ack })) => {
                    if ack {
                        received_ack = true;
                    } else {
                        received_settings = true;
                    }
                    if received_settings && received_ack {
                        break;
                    }
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(received_settings);
        assert!(received_ack);
    });

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    let mut received_settings = false;
    let mut received_ack = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            Http2Event::SettingsReceived { ack } => {
                if ack {
                    received_ack = true;
                } else {
                    received_settings = true;
                }
                if received_settings && received_ack {
                    break;
                }
            }
            Http2Event::ConnectionPreface => {}
            _ => {}
        }
    }

    assert!(received_settings);
    assert!(received_ack);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// PING テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: PING/PONG
#[tokio::test]
async fn test_nghttp2_client_http2_server_ping() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::PingReceived { ack, .. })) => {
                    if !ack {
                        conn.flush().await.expect("failed to flush");
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let ping_data = [1, 2, 3, 4, 5, 6, 7, 8];
    client.ping(&ping_data).await.expect("failed to send ping");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::PingReceived { opaque_data, ack } = event {
            assert!(ack);
            assert_eq!(opaque_data, ping_data);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.abort();
}

/// http2 クライアント <-> nghttp2 サーバー: PING/PONG
#[tokio::test]
async fn test_http2_client_nghttp2_server_ping() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::PingReceived { ack, .. })) => {
                    if !ack {
                        conn.flush().await.expect("failed to flush");
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let ping_data = [1, 2, 3, 4, 5, 6, 7, 8];
    client.ping(ping_data).await.expect("failed to send ping");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::PingReceived { opaque_data, ack } = event {
            assert!(ack);
            assert_eq!(opaque_data, ping_data);
            break;
        }
    }

    client.shutdown().await.ok();
    server_handle.abort();
}

// ============================================================================
// GOAWAY テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: GOAWAY
#[tokio::test]
async fn test_nghttp2_client_http2_server_goaway() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::SettingsReceived { ack: true })) => break,
                Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => return,
            }
        }

        conn.shutdown().await.expect("failed to send goaway");
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::GoawayReceived {
            last_stream_id,
            error_code,
            ..
        } = event
        {
            assert_eq!(last_stream_id, 0);
            assert_eq!(error_code, NgErrorCode::NoError);
            break;
        }
    }

    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: GOAWAY
#[tokio::test]
async fn test_http2_client_nghttp2_server_goaway() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::SettingsReceived { ack: true })) => break,
                Ok(Ok(_)) => {}
                _ => return,
            }
        }

        conn.shutdown(0).await.expect("failed to send goaway");
        conn.flush().await.expect("failed to flush");
    });

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::GoawayReceived {
            last_stream_id,
            error_code,
            ..
        } = event
        {
            assert_eq!(last_stream_id, tokio_http2::StreamId::Connection);
            assert_eq!(error_code, Http2ErrorCode::NoError);
            break;
        }
    }

    let _ = server_handle.await;
}

// ============================================================================
// POST リクエスト (DATA フレーム) テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: POST リクエストとレスポンスボディ
#[tokio::test]
async fn test_nghttp2_client_http2_server_post_with_body() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut request_body = Vec::new();
        // ヘッダー受信後に設定される
        let mut request_stream_id: Option<tokio_http2::StreamId> = None;
        let mut headers_received = false;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    let method = headers
                        .iter()
                        .find(|h| h.name() == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value(), b"POST");
                    request_stream_id = Some(stream_id);
                    headers_received = true;

                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(Http2Event::DataReceived {
                    stream_id,
                    data,
                    end_stream,
                })) => {
                    if headers_received {
                        assert_eq!(
                            stream_id,
                            request_stream_id.expect("data received before headers")
                        );
                        request_body.extend_from_slice(&data);

                        if end_stream {
                            break;
                        }
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        // リクエストボディを検証
        assert!(headers_received);
        assert_eq!(request_body, b"Hello from nghttp2!");

        // レスポンスを送信
        let sid = request_stream_id.expect("headers not received");
        let response_headers =
            vec![HeaderField::new(":status", "200").expect("valid header field")];
        conn.send_response(sid, response_headers, true)
            .await
            .expect("failed to send response");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    // POST リクエストを送信 (ボディ付き)
    let request_headers = vec![
        NgHeader::method("POST"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
        NgHeader::new(b"content-type".to_vec(), b"text/plain".to_vec()),
    ];

    let stream_id = client
        .send_request(&request_headers, Some(b"Hello from nghttp2!"), true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: POST リクエストとレスポンスボディ
#[tokio::test]
async fn test_http2_client_nghttp2_server_post_with_body() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut request_body = Vec::new();
        let mut request_stream_id = 0;
        let mut headers_received = false;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    let method = headers
                        .iter()
                        .find(|h| h.name == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value, b"POST");
                    request_stream_id = stream_id;
                    headers_received = true;

                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::DataReceived {
                    stream_id,
                    data,
                    end_stream,
                })) => {
                    if headers_received {
                        assert_eq!(stream_id, request_stream_id);
                        request_body.extend_from_slice(&data);

                        if end_stream {
                            break;
                        }
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. }))
                | Ok(Ok(NgHttp2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        // リクエストボディを検証
        assert_eq!(request_body, b"Hello, Server!");

        // レスポンスヘッダー送信 (end_stream=false でボディを後で送信)
        let response_headers = vec![NgHeader::status(200)];
        conn.send_response(request_stream_id, &response_headers, false)
            .await
            .expect("failed to send response");

        // レスポンスボディ送信
        conn.send_data(request_stream_id, b"Echo: Hello, Server!", true)
            .await
            .expect("failed to send data");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    // POST リクエストを送信（ヘッダー）
    let request_headers = vec![
        HeaderField::new(":method", "POST").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new("content-type", "text/plain").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request headers");

    // リクエストボディを送信
    client
        .send_data(stream_id, b"Hello, Server!".to_vec(), true)
        .await
        .expect("failed to send request body");

    // レスポンスヘッダーとボディを待機
    let mut received_headers = false;
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(!end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value(), b"200");
                received_headers = true;
            }
            Http2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_body.extend_from_slice(&data);

                if end_stream {
                    break;
                }
            }
            _ => {}
        }
    }

    assert!(received_headers);
    assert_eq!(received_body, b"Echo: Hello, Server!");

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// レスポンスボディ (DATA フレーム) テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: レスポンスボディ受信
#[tokio::test]
async fn test_nghttp2_client_http2_server_response_body() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let response_body = b"This is the response body from the server.";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // レスポンスヘッダーを送信
                        let response_headers = vec![
                            HeaderField::new(":status", "200").expect("valid header field"),
                            HeaderField::new("content-type", "text/plain")
                                .expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        // レスポンスボディを送信
                        conn.send_data(stream_id, response_body.to_vec(), true)
                            .await
                            .expect("failed to send response body");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    let mut received_headers = false;
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(!end_stream); // ボディが続く

                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value, b"200");
                received_headers = true;
            }
            NgHttp2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_body.extend_from_slice(&data);

                if end_stream {
                    break;
                }
            }
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    assert!(received_headers);
    assert_eq!(received_body, response_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: レスポンスボディ受信
#[tokio::test]
async fn test_http2_client_nghttp2_server_response_body() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let response_body = b"response from nghttp2 server";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // レスポンスヘッダー送信 (end_stream=false でボディを後で送信)
                        let response_headers = vec![NgHeader::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");

                        // レスポンスボディ送信
                        conn.send_data(stream_id, b"response from nghttp2 server", true)
                            .await
                            .expect("failed to send data");

                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_headers = false;
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(!end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value(), b"200");
                received_headers = true;
            }
            Http2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_body.extend_from_slice(&data);

                if end_stream {
                    break;
                }
            }
            _ => {}
        }
    }

    assert!(received_headers);
    assert_eq!(received_body, response_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 様々な HTTP メソッドテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: PUT メソッド
#[tokio::test]
async fn test_nghttp2_client_http2_server_put() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name() == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value(), b"PUT");

                        let response_headers =
                            vec![HeaderField::new(":status", "204").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("PUT"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/resource"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"204");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: DELETE メソッド
#[tokio::test]
async fn test_http2_client_nghttp2_server_delete() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value, b"DELETE");

                        let response_headers = vec![NgHeader::status(204)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "DELETE").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/resource/123").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"204");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 クライアント <-> http2 サーバー: HEAD メソッド
#[tokio::test]
async fn test_nghttp2_client_http2_server_head() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name() == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value(), b"HEAD");

                        // HEAD レスポンスはボディなし
                        let response_headers = vec![
                            HeaderField::new(":status", "200").expect("valid header field"),
                            HeaderField::new("content-length", "1234").expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("HEAD"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream); // HEAD はボディなし

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");

            let content_length = headers
                .iter()
                .find(|h| h.name == b"content-length")
                .expect("missing content-length header");
            assert_eq!(content_length.value, b"1234");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 様々な HTTP ステータスコードテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 404 Not Found
#[tokio::test]
async fn test_nghttp2_client_http2_server_404() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "404").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/not-found"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"404");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 500 Internal Server Error
#[tokio::test]
async fn test_http2_client_nghttp2_server_500() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![NgHeader::status(500)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/error").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"500");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// カスタムヘッダーテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: カスタムヘッダー
#[tokio::test]
async fn test_nghttp2_client_http2_server_custom_headers() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // カスタムヘッダーを検証
                        let x_custom = headers
                            .iter()
                            .find(|h| h.name() == b"x-custom-header")
                            .expect("missing x-custom-header");
                        assert_eq!(x_custom.value(), b"custom-value");

                        let x_request_id = headers
                            .iter()
                            .find(|h| h.name() == b"x-request-id")
                            .expect("missing x-request-id");
                        assert_eq!(x_request_id.value(), b"12345");

                        // レスポンスにもカスタムヘッダーを付与
                        let response_headers = vec![
                            HeaderField::new(":status", "200").expect("valid header field"),
                            HeaderField::new("x-response-id", "67890").expect("valid header field"),
                            HeaderField::new("x-server", "test-server")
                                .expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
        NgHeader::new(b"x-custom-header".to_vec(), b"custom-value".to_vec()),
        NgHeader::new(b"x-request-id".to_vec(), b"12345".to_vec()),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");

            let x_response_id = headers
                .iter()
                .find(|h| h.name == b"x-response-id")
                .expect("missing x-response-id header");
            assert_eq!(x_response_id.value, b"67890");

            let x_server = headers
                .iter()
                .find(|h| h.name == b"x-server")
                .expect("missing x-server header");
            assert_eq!(x_server.value, b"test-server");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: カスタムヘッダー
#[tokio::test]
async fn test_http2_client_nghttp2_server_custom_headers() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // カスタムヘッダーを検証
                        let x_custom = headers
                            .iter()
                            .find(|h| h.name == b"x-custom-header")
                            .expect("missing x-custom-header");
                        assert_eq!(x_custom.value, b"custom-value");

                        // レスポンスにもカスタムヘッダーを付与
                        let response_headers = vec![
                            NgHeader::status(200),
                            NgHeader::new(b"x-response-id".to_vec(), b"67890".to_vec()),
                        ];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new("x-custom-header", "custom-value").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");

            let x_response_id = headers
                .iter()
                .find(|h| h.name() == b"x-response-id")
                .expect("missing x-response-id header");
            assert_eq!(x_response_id.value(), b"67890");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 様々な RST_STREAM エラーコードテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: RST_STREAM InternalError
#[tokio::test]
async fn test_nghttp2_client_http2_server_rst_stream_internal_error() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        conn.reset_stream(stream_id, Http2ErrorCode::InternalError)
                            .await
                            .expect("failed to send rst_stream");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::StreamClosed {
            stream_id: closed_stream_id,
            error_code,
        } = event
        {
            assert_eq!(closed_stream_id, stream_id);
            assert_eq!(error_code, NgErrorCode::InternalError);
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: RST_STREAM RefusedStream
#[tokio::test]
async fn test_http2_client_nghttp2_server_rst_stream_refused() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        conn.reset_stream(stream_id, NgErrorCode::RefusedStream)
                            .await
                            .expect("failed to send rst_stream");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::StreamReset {
            stream_id: reset_stream_id,
            error_code,
        } = event
        {
            assert_eq!(reset_stream_id, stream_id);
            assert_eq!(error_code, Http2ErrorCode::RefusedStream);
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 大きなデータ転送テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 中程度のレスポンスボディ
#[tokio::test]
async fn test_nghttp2_client_http2_server_large_response() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    // 16KB のレスポンスボディ（初期ウィンドウサイズ内）
    let response_body: Vec<u8> = (0..16384).map(|i| (i % 256) as u8).collect();
    let response_body_clone = response_body.clone();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        conn.send_data(stream_id, response_body_clone.clone(), true)
                            .await
                            .expect("failed to send response body");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/large"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(10), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            NgHttp2Event::HeadersReceived { .. } => {}
            NgHttp2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_body.extend_from_slice(&data);

                if end_stream {
                    break;
                }
            }
            NgHttp2Event::SettingsReceived { .. } | NgHttp2Event::WindowUpdateReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_body.len(), response_body.len());
    assert_eq!(received_body, response_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 中程度のリクエストボディ
#[tokio::test]
async fn test_http2_client_nghttp2_server_large_request() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    // 16KB のリクエストボディ（初期ウィンドウサイズ内）
    let request_body: Vec<u8> = (0..16384).map(|i| (i % 256) as u8).collect();
    let request_body_clone = request_body.clone();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut received_body = Vec::new();
        let mut request_stream_id = 0;

        loop {
            match tokio::time::timeout(Duration::from_secs(10), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    request_stream_id = stream_id;
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::DataReceived {
                    data, end_stream, ..
                })) => {
                    received_body.extend_from_slice(&data);
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. }))
                | Ok(Ok(NgHttp2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        // リクエストボディを検証
        assert_eq!(received_body.len(), request_body_clone.len());
        assert_eq!(received_body, request_body_clone);

        // レスポンスを送信
        let response_headers = vec![NgHeader::status(200)];
        conn.send_response(request_stream_id, &response_headers, true)
            .await
            .expect("failed to send response");
        conn.flush().await.expect("failed to flush");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "POST").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/upload").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request headers");

    client
        .send_data(stream_id, request_body, true)
        .await
        .expect("failed to send request body");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(10), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// クライアントからの RST_STREAM テスト
// ============================================================================

/// nghttp2 クライアントからの RST_STREAM -> http2 サーバー
#[tokio::test]
async fn test_nghttp2_client_rst_stream_to_http2_server() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived { stream_id, .. })) => {
                    // レスポンスを送信開始（end_stream=false）
                    let response_headers =
                        vec![HeaderField::new(":status", "200").expect("valid header field")];
                    conn.send_response(stream_id, response_headers, false)
                        .await
                        .expect("failed to send response");
                    // データ送信前にクライアントが RST_STREAM を送る
                }
                Ok(Ok(Http2Event::StreamReset {
                    stream_id: _,
                    error_code,
                })) => {
                    // クライアントからの RST_STREAM を受信
                    assert_eq!(error_code, Http2ErrorCode::Cancel);
                    break;
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    // レスポンスヘッダーを受信したらキャンセル
    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            // RST_STREAM を送信してキャンセル
            // nghttp2 の Client には直接 reset_stream がないため、
            // ここでは Connection を使う必要がある
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 複数接続テスト
// ============================================================================

/// nghttp2 クライアント複数 <-> http2 サーバー: 同時接続
#[tokio::test]
async fn test_multiple_nghttp2_clients_http2_server() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    // サーバータスク（2つの接続を処理）
    let server_handle = tokio::spawn(async move {
        for _ in 0..2 {
            let mut conn = server.accept().await.expect("failed to accept connection");

            tokio::spawn(async move {
                loop {
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(Http2Event::HeadersReceived {
                            stream_id,
                            end_stream,
                            ..
                        })) => {
                            if end_stream {
                                let response_headers = vec![
                                    HeaderField::new(":status", "200").expect("valid header field"),
                                ];
                                conn.send_response(stream_id, response_headers, true)
                                    .await
                                    .expect("failed to send response");
                            }
                        }
                        Ok(Ok(Http2Event::SettingsReceived { .. }))
                        | Ok(Ok(Http2Event::ConnectionPreface))
                        | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                        Ok(Ok(_)) => {}
                        _ => break,
                    }
                }
            });
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2つのクライアントを同時に接続
    let client1_handle = tokio::spawn(async move {
        let mut client = NgClient::connect_insecure(server_addr, "localhost")
            .await
            .expect("client1: failed to connect");

        wait_for_nghttp2_settings_ack(&mut client).await;

        let request_headers = vec![
            NgHeader::method("GET"),
            NgHeader::scheme("https"),
            NgHeader::authority("localhost"),
            NgHeader::path("/client1"),
        ];
        let stream_id = client
            .send_request(&request_headers, None, true)
            .await
            .expect("client1: failed to send request");
        client.flush().await.expect("client1: failed to flush");

        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => panic!("client1: timeout"),
                };
            if let NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                ..
            } = event
            {
                assert_eq!(recv_stream_id, stream_id);
                break;
            }
        }
        client.shutdown().await.ok();
    });

    let client2_handle = tokio::spawn(async move {
        let mut client = NgClient::connect_insecure(server_addr, "localhost")
            .await
            .expect("client2: failed to connect");

        wait_for_nghttp2_settings_ack(&mut client).await;

        let request_headers = vec![
            NgHeader::method("GET"),
            NgHeader::scheme("https"),
            NgHeader::authority("localhost"),
            NgHeader::path("/client2"),
        ];
        let stream_id = client
            .send_request(&request_headers, None, true)
            .await
            .expect("client2: failed to send request");
        client.flush().await.expect("client2: failed to flush");

        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => panic!("client2: timeout"),
                };
            if let NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                ..
            } = event
            {
                assert_eq!(recv_stream_id, stream_id);
                break;
            }
        }
        client.shutdown().await.ok();
    });

    let (r1, r2) = tokio::join!(client1_handle, client2_handle);
    r1.expect("client1 failed");
    r2.expect("client2 failed");

    server_handle.abort();
}

// ============================================================================
// レスポンスヘッダー + ボディ + END_STREAM 分離テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: レスポンスヘッダーとボディを分離送信
/// send_response(end_stream=false) → send_data(end_stream=true) の 2 段階
#[tokio::test]
async fn test_nghttp2_client_http2_server_response_headers_then_data() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let response_body = b"response body after headers";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // ヘッダーのみ送信 (end_stream=false)
                        let response_headers = vec![
                            HeaderField::new(":status", "200").expect("valid header field"),
                            HeaderField::new("content-type", "text/plain")
                                .expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        // ボディ送信 (end_stream=true)
                        conn.send_data(stream_id, response_body.to_vec(), true)
                            .await
                            .expect("failed to send response body");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    let mut received_headers = false;
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(!end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value, b"200");
                received_headers = true;
            }
            NgHttp2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_body.extend_from_slice(&data);

                if end_stream {
                    break;
                }
            }
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    assert!(received_headers);
    assert_eq!(received_body, response_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> http2 サーバー: レスポンスヘッダーとボディを分離送信
#[tokio::test]
async fn test_http2_client_http2_server_response_headers_then_data() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let response_body = b"response body after headers";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![
                            HeaderField::new(":status", "200").expect("valid header field"),
                            HeaderField::new("content-type", "text/plain")
                                .expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        conn.send_data(stream_id, response_body.to_vec(), true)
                            .await
                            .expect("failed to send response body");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client =
        tokio_http2::Client::connect_insecure(server_addr, "localhost", Limits::default())
            .await
            .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_headers = false;
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(!end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value(), b"200");
                received_headers = true;
            }
            Http2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_body.extend_from_slice(&data);

                if end_stream {
                    break;
                }
            }
            Http2Event::SettingsReceived { .. } | Http2Event::ConnectionPreface => {}
            _ => {}
        }
    }

    assert!(received_headers);
    assert_eq!(received_body, response_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 複数 DATA フレーム分割送信テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 複数 DATA フレーム分割送信
/// 1 つのストリームで複数回 send_data を呼び、最後にのみ end_stream=true
#[tokio::test]
async fn test_nghttp2_client_http2_server_multiple_data_frames() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let chunks: Vec<&[u8]> = vec![b"chunk1-", b"chunk2-", b"chunk3"];
    let expected_body: Vec<u8> = chunks.iter().flat_map(|c| c.iter()).copied().collect();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        // 3 回に分けてデータ送信
                        conn.send_data(stream_id, b"chunk1-".to_vec(), false)
                            .await
                            .expect("failed to send chunk1");
                        conn.send_data(stream_id, b"chunk2-".to_vec(), false)
                            .await
                            .expect("failed to send chunk2");
                        conn.send_data(stream_id, b"chunk3".to_vec(), true)
                            .await
                            .expect("failed to send chunk3");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let _stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for data"),
        };
        match event {
            NgHttp2Event::HeadersReceived { .. } => {}
            NgHttp2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_body, expected_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: リクエストボディ複数 DATA フレーム分割送信
#[tokio::test]
async fn test_http2_client_nghttp2_server_multiple_data_frames() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let expected_body: Vec<u8> = b"chunk1-chunk2-chunk3".to_vec();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut received_body = Vec::new();
        let mut request_stream_id = 0;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    request_stream_id = stream_id;
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::DataReceived {
                    data, end_stream, ..
                })) => {
                    received_body.extend_from_slice(&data);
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. }))
                | Ok(Ok(NgHttp2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert_eq!(received_body, expected_body);

        let response_headers = vec![NgHeader::status(200)];
        conn.send_response(request_stream_id, &response_headers, true)
            .await
            .expect("failed to send response");
        conn.flush().await.expect("failed to flush");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "POST").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request headers");

    // 3 回に分けてリクエストボディ送信
    client
        .send_data(stream_id, b"chunk1-".to_vec(), false)
        .await
        .expect("failed to send chunk1");
    client
        .send_data(stream_id, b"chunk2-".to_vec(), false)
        .await
        .expect("failed to send chunk2");
    client
        .send_data(stream_id, b"chunk3".to_vec(), true)
        .await
        .expect("failed to send chunk3");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 空ボディの POST (content-length: 0) テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 空ボディの POST
#[tokio::test]
async fn test_nghttp2_client_http2_server_post_empty_body() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    let method = headers
                        .iter()
                        .find(|h| h.name() == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value(), b"POST");

                    let content_length = headers
                        .iter()
                        .find(|h| h.name() == b"content-length")
                        .expect("missing content-length header");
                    assert_eq!(content_length.value(), b"0");

                    // end_stream=true であることを確認 (ボディなし)
                    assert!(end_stream);

                    let response_headers =
                        vec![HeaderField::new(":status", "200").expect("valid header field")];
                    conn.send_response(stream_id, response_headers, true)
                        .await
                        .expect("failed to send response");
                    break;
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("POST"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
        NgHeader::new(b"content-length".to_vec(), b"0".to_vec()),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 空ボディの POST
#[tokio::test]
async fn test_http2_client_nghttp2_server_post_empty_body() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    let method = headers
                        .iter()
                        .find(|h| h.name == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value, b"POST");

                    let content_length = headers
                        .iter()
                        .find(|h| h.name == b"content-length")
                        .expect("missing content-length header");
                    assert_eq!(content_length.value, b"0");

                    assert!(end_stream);

                    let response_headers = vec![NgHeader::status(200)];
                    conn.send_response(stream_id, &response_headers, true)
                        .await
                        .expect("failed to send response");
                    conn.flush().await.expect("failed to flush");
                    break;
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "POST").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new("content-length", "0").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// レスポンスボディ付き 404/500 テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 404 レスポンスにボディ付き
#[tokio::test]
async fn test_nghttp2_client_http2_server_404_with_body() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let error_body = b"Not Found: the requested resource does not exist";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![
                            HeaderField::new(":status", "404").expect("valid header field"),
                            HeaderField::new("content-type", "text/plain")
                                .expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        conn.send_data(stream_id, error_body.to_vec(), true)
                            .await
                            .expect("failed to send error body");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/not-found"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    let mut received_status = Vec::new();
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                received_status = status.value.clone();
            }
            NgHttp2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_status, b"404");
    assert_eq!(received_body, error_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 クライアント <-> http2 サーバー: 500 レスポンスにボディ付き
#[tokio::test]
async fn test_nghttp2_client_http2_server_500_with_body() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let error_body = b"Internal Server Error: an unexpected error occurred";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![
                            HeaderField::new(":status", "500").expect("valid header field"),
                            HeaderField::new("content-type", "text/plain")
                                .expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        conn.send_data(stream_id, error_body.to_vec(), true)
                            .await
                            .expect("failed to send error body");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/error"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    let mut received_status = Vec::new();
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                received_status = status.value.clone();
            }
            NgHttp2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_status, b"500");
    assert_eq!(received_body, error_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// OPTIONS メソッドテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: OPTIONS メソッド
#[tokio::test]
async fn test_nghttp2_client_http2_server_options() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name() == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value(), b"OPTIONS");

                        let response_headers = vec![
                            HeaderField::new(":status", "204").expect("valid header field"),
                            HeaderField::new("allow", "GET, POST, OPTIONS")
                                .expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("OPTIONS"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"204");

            let allow = headers
                .iter()
                .find(|h| h.name == b"allow")
                .expect("missing allow header");
            assert_eq!(allow.value, b"GET, POST, OPTIONS");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: OPTIONS メソッド
#[tokio::test]
async fn test_http2_client_nghttp2_server_options() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value, b"OPTIONS");

                        let response_headers = vec![
                            NgHeader::status(204),
                            NgHeader::new(b"allow".to_vec(), b"GET, POST, OPTIONS".to_vec()),
                        ];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "OPTIONS").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"204");

            let allow = headers
                .iter()
                .find(|h| h.name() == b"allow")
                .expect("missing allow header");
            assert_eq!(allow.value(), b"GET, POST, OPTIONS");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 多数ヘッダーテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 20 個のカスタムヘッダー
#[tokio::test]
async fn test_nghttp2_client_http2_server_many_headers() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // 20 個のカスタムヘッダーが到着しているか検証
                        for i in 0..20 {
                            let name = format!("x-header-{}", i);
                            let expected_value = format!("value-{}", i);
                            let header = headers
                                .iter()
                                .find(|h| h.name() == name.as_bytes())
                                .unwrap_or_else(|| panic!("missing {}", name));
                            assert_eq!(header.value(), expected_value.as_bytes());
                        }

                        // レスポンスにも 20 個のカスタムヘッダーを付与
                        let mut response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        for i in 0..20 {
                            response_headers.push(
                                HeaderField::new(
                                    format!("x-resp-{}", i),
                                    format!("resp-value-{}", i),
                                )
                                .expect("should succeed"),
                            );
                        }
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let mut request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    for i in 0..20 {
        request_headers.push(NgHeader::new(
            format!("x-header-{}", i).into_bytes(),
            format!("value-{}", i).into_bytes(),
        ));
    }

    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");

            for i in 0..20 {
                let name = format!("x-resp-{}", i);
                let expected_value = format!("resp-value-{}", i);
                let header = headers
                    .iter()
                    .find(|h| h.name == name.as_bytes())
                    .unwrap_or_else(|| panic!("missing {}", name));
                assert_eq!(header.value, expected_value.as_bytes());
            }
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 20 個のカスタムヘッダー
#[tokio::test]
async fn test_http2_client_nghttp2_server_many_headers() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        for i in 0..20 {
                            let name = format!("x-header-{}", i);
                            let expected_value = format!("value-{}", i);
                            let header = headers
                                .iter()
                                .find(|h| h.name == name.as_bytes())
                                .unwrap_or_else(|| panic!("missing {}", name));
                            assert_eq!(header.value, expected_value.as_bytes());
                        }

                        let mut response_headers = vec![NgHeader::status(200)];
                        for i in 0..20 {
                            response_headers.push(NgHeader::new(
                                format!("x-resp-{}", i).into_bytes(),
                                format!("resp-value-{}", i).into_bytes(),
                            ));
                        }
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let mut request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    for i in 0..20 {
        request_headers.push(
            HeaderField::new(format!("x-header-{}", i), format!("value-{}", i))
                .expect("valid header field"),
        );
    }

    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");

            for i in 0..20 {
                let name = format!("x-resp-{}", i);
                let expected_value = format!("resp-value-{}", i);
                let header = headers
                    .iter()
                    .find(|h| h.name() == name.as_bytes())
                    .unwrap_or_else(|| panic!("missing {}", name));
                assert_eq!(header.value(), expected_value.as_bytes());
            }
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// GOAWAY 後の既存ストリーム処理テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: GOAWAY 後に既存ストリームを完了
#[tokio::test]
async fn test_nghttp2_client_http2_server_goaway_after_stream() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        // ヘッダー受信後に設定される
        let mut received_stream_id: Option<tokio_http2::StreamId> = None;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        received_stream_id = Some(stream_id);

                        // まずレスポンスを送信
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");

                        // その後 GOAWAY を送信
                        conn.shutdown().await.expect("failed to send goaway");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        // ストリームを受信したことを確認 (Connection ではないこと)
        assert!(received_stream_id.is_some());
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    let mut received_response = false;
    let mut received_goaway = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value, b"200");
                received_response = true;
            }
            NgHttp2Event::GoawayReceived {
                last_stream_id,
                error_code,
                ..
            } => {
                // last_stream_id はクライアントが開始したストリーム ID 以上
                assert!(last_stream_id >= stream_id);
                assert_eq!(error_code, NgErrorCode::NoError);
                received_goaway = true;
            }
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }

        if received_response && received_goaway {
            break;
        }
    }

    assert!(received_response);
    assert!(received_goaway);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: GOAWAY 後に既存ストリームを完了
#[tokio::test]
async fn test_http2_client_nghttp2_server_goaway_after_stream() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // まずレスポンスを送信
                        let response_headers = vec![NgHeader::status(200)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");

                        // その後 GOAWAY を送信
                        conn.shutdown(stream_id)
                            .await
                            .expect("failed to send goaway");
                        conn.flush().await.expect("failed to flush goaway");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_response = false;
    let mut received_goaway = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                assert!(end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value(), b"200");
                received_response = true;
            }
            Http2Event::GoawayReceived {
                last_stream_id,
                error_code,
                ..
            } => {
                // last_stream_id はクライアントが開始したストリーム ID 以上
                assert!(last_stream_id.as_u32() >= stream_id.as_u32());
                assert_eq!(error_code, Http2ErrorCode::NoError);
                received_goaway = true;
            }
            Http2Event::SettingsReceived { .. } | Http2Event::ConnectionPreface => {}
            _ => {}
        }

        if received_response && received_goaway {
            break;
        }
    }

    assert!(received_response);
    assert!(received_goaway);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 双方向データ転送テスト
// ============================================================================

/// http2 クライアント <-> http2 サーバー: リクエスト・レスポンス両方にボディ
#[tokio::test]
async fn test_http2_client_http2_server_bidirectional_data() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let request_body_expected = b"request body from client";
    let response_body = b"response body from server";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        // ヘッダー受信後に設定される
        let mut request_stream_id: Option<tokio_http2::StreamId> = None;
        let mut received_body = Vec::new();

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    request_stream_id = Some(stream_id);
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(Http2Event::DataReceived {
                    data, end_stream, ..
                })) => {
                    received_body.extend_from_slice(&data);
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert_eq!(received_body, request_body_expected);

        let sid = request_stream_id.expect("headers not received");
        let resp_headers = vec![HeaderField::new(":status", "200").expect("valid header field")];
        conn.send_response(sid, resp_headers, false)
            .await
            .expect("failed to send response headers");

        conn.send_data(sid, response_body.to_vec(), true)
            .await
            .expect("failed to send response body");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client =
        tokio_http2::Client::connect_insecure(server_addr, "localhost", Limits::default())
            .await
            .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "POST").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request headers");

    client
        .send_data(stream_id, request_body_expected.to_vec(), true)
        .await
        .expect("failed to send request body");

    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value(), b"200");

                if end_stream {
                    break;
                }
            }
            Http2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::SettingsReceived { .. } | Http2Event::ConnectionPreface => {}
            _ => {}
        }
    }

    assert_eq!(received_body, response_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: リクエストボディ送信しレスポンスはヘッダーのみ
/// (nghttp2 サーバーは send_data 未対応のためレスポンスボディなし)
#[tokio::test]
async fn test_http2_client_nghttp2_server_bidirectional_data() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let request_body_expected = b"bidirectional request body";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut request_stream_id = 0;
        let mut received_body = Vec::new();

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    request_stream_id = stream_id;
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::DataReceived {
                    data, end_stream, ..
                })) => {
                    received_body.extend_from_slice(&data);
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. }))
                | Ok(Ok(NgHttp2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert_eq!(received_body, request_body_expected);

        let response_headers = vec![NgHeader::status(200)];
        conn.send_response(request_stream_id, &response_headers, true)
            .await
            .expect("failed to send response");
        conn.flush().await.expect("failed to flush");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "POST").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request headers");

    client
        .send_data(stream_id, request_body_expected.to_vec(), true)
        .await
        .expect("failed to send request body");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// ストリームリセット後の別ストリーム継続テスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 1 ストリームをリセットし他方は継続
#[tokio::test]
async fn test_nghttp2_client_http2_server_rst_one_stream_continue_other() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut streams_received = Vec::new();

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let path = headers
                            .iter()
                            .find(|h| h.name() == b":path")
                            .expect("missing :path header");

                        streams_received.push((stream_id, path.value().to_vec()));

                        if streams_received.len() == 2 {
                            // /reset パスのストリームを RST_STREAM
                            // /ok パスのストリームには正常レスポンス
                            for (sid, p) in &streams_received {
                                if p == b"/reset" {
                                    conn.reset_stream(*sid, Http2ErrorCode::Cancel)
                                        .await
                                        .expect("failed to send rst_stream");
                                } else {
                                    let response_headers = vec![
                                        HeaderField::new(":status", "200")
                                            .expect("valid header field"),
                                    ];
                                    conn.send_response(*sid, response_headers, true)
                                        .await
                                        .expect("failed to send response");
                                }
                            }
                            break;
                        }
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    // 2 本のストリームを送信
    let reset_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/reset"),
    ];
    let reset_stream_id = client
        .send_request(&reset_headers, None, true)
        .await
        .expect("failed to send request");

    let ok_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/ok"),
    ];
    let ok_stream_id = client
        .send_request(&ok_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    let mut got_reset = false;
    let mut got_response = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            NgHttp2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, ok_stream_id);
                assert!(end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value, b"200");
                got_response = true;
            }
            NgHttp2Event::StreamClosed {
                stream_id: closed_stream_id,
                error_code,
            } => {
                if closed_stream_id == reset_stream_id {
                    assert_eq!(error_code, NgErrorCode::Cancel);
                    got_reset = true;
                }
            }
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }

        if got_reset && got_response {
            break;
        }
    }

    assert!(got_reset);
    assert!(got_response);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 1 ストリームをリセットし他方は継続
#[tokio::test]
async fn test_http2_client_nghttp2_server_rst_one_stream_continue_other() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut streams_received = Vec::new();

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let path = headers
                            .iter()
                            .find(|h| h.name == b":path")
                            .expect("missing :path header");

                        streams_received.push((stream_id, path.value.clone()));

                        if streams_received.len() == 2 {
                            for (sid, p) in &streams_received {
                                if p == b"/reset" {
                                    conn.reset_stream(*sid, NgErrorCode::Cancel)
                                        .await
                                        .expect("failed to send rst_stream");
                                } else {
                                    let response_headers = vec![NgHeader::status(200)];
                                    conn.send_response(*sid, &response_headers, true)
                                        .await
                                        .expect("failed to send response");
                                }
                            }
                            conn.flush().await.expect("failed to flush");
                            break;
                        }
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let reset_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/reset").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let reset_stream_id = client
        .send_request(reset_headers, true)
        .await
        .expect("failed to send request");

    let ok_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/ok").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let ok_stream_id = client
        .send_request(ok_headers, true)
        .await
        .expect("failed to send request");

    let mut got_reset = false;
    let mut got_response = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(recv_stream_id, ok_stream_id);
                assert!(end_stream);

                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                assert_eq!(status.value(), b"200");
                got_response = true;
            }
            Http2Event::StreamReset {
                stream_id: rst_stream_id,
                error_code,
            } => {
                if rst_stream_id == reset_stream_id {
                    assert_eq!(error_code, Http2ErrorCode::Cancel);
                    got_reset = true;
                }
            }
            Http2Event::SettingsReceived { .. } | Http2Event::ConnectionPreface => {}
            _ => {}
        }

        if got_reset && got_response {
            break;
        }
    }

    assert!(got_reset);
    assert!(got_response);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 204 No Content レスポンステスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 204 No Content
#[tokio::test]
async fn test_nghttp2_client_http2_server_204_no_content() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // 204 はボディなし (end_stream=true)
                        let response_headers =
                            vec![HeaderField::new(":status", "204").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("DELETE"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/resource/1"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"204");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 204 No Content
#[tokio::test]
async fn test_http2_client_nghttp2_server_204_no_content() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![NgHeader::status(204)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "DELETE").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/resource/1").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"204");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 嫌がらせ系 interop テスト
// ============================================================================

#[allow(clippy::single_match, clippy::collapsible_if)]
mod stress_tests {
    use super::*;

    /// nghttp2 クライアント <-> http2 サーバー: 大量同時ストリーム
    #[tokio::test]
    async fn test_nghttp2_client_http2_server_many_concurrent_streams() {
        let tls_config = generate_http2_test_cert();
        let limits = Limits::default();

        let server = Http2Server::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
            limits,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let stream_count = 30;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");
            let mut responded = 0;

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    Http2Event::HeadersReceived {
                        stream_id,
                        end_stream,
                        ..
                    } => {
                        if end_stream {
                            let response_headers = vec![
                                HeaderField::new(":status", "200").expect("valid header field"),
                            ];
                            conn.send_response(stream_id, response_headers, true)
                                .await
                                .expect("failed to send response");
                            responded += 1;
                            if responded >= stream_count {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        let mut client = NgClient::connect_insecure(server_addr, "localhost")
            .await
            .expect("failed to connect");

        wait_for_nghttp2_settings_ack(&mut client).await;

        let mut stream_ids = Vec::new();
        for i in 0..stream_count {
            let request_headers = vec![
                NgHeader::method("GET"),
                NgHeader::scheme("https"),
                NgHeader::authority("localhost"),
                NgHeader::path(&format!("/path/{}", i)),
            ];
            let stream_id = client
                .send_request(&request_headers, None, true)
                .await
                .expect("failed to send request");
            stream_ids.push(stream_id);
        }

        let mut responses = 0;
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            if let NgHttp2Event::HeadersReceived { end_stream, .. } = event {
                if end_stream {
                    responses += 1;
                    if responses >= stream_count {
                        break;
                    }
                }
            }
        }
        assert_eq!(responses, stream_count);

        client.shutdown().await.ok();
        let _ = server_handle.await;
    }

    /// http2 クライアント <-> nghttp2 サーバー: 大量同時ストリーム
    #[tokio::test]
    async fn test_http2_client_nghttp2_server_many_concurrent_streams() {
        let tls_config = generate_nghttp2_test_cert();

        let server = NgServer::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let stream_count = 30;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");
            let mut responded = 0;

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    NgHttp2Event::HeadersReceived {
                        stream_id,
                        end_stream,
                        ..
                    } => {
                        if end_stream {
                            let response_headers = vec![NgHeader::status(200)];
                            conn.send_response(stream_id, &response_headers, true)
                                .await
                                .expect("failed to send response");
                            responded += 1;
                            if responded >= stream_count {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        let limits = Limits::default();
        let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
            .await
            .expect("failed to connect");

        wait_for_http2_settings_ack(&mut client).await;

        let mut stream_ids = Vec::new();
        for i in 0..stream_count {
            let request_headers = vec![
                HeaderField::new(":method", "GET").expect("valid header field"),
                HeaderField::new(":scheme", "https").expect("valid header field"),
                HeaderField::new(":authority", "localhost").expect("valid header field"),
                HeaderField::new(":path", format!("/path/{}", i)).expect("valid header field"),
            ];
            let stream_id = client
                .send_request(request_headers, true)
                .await
                .expect("failed to send request");
            stream_ids.push(stream_id);
        }

        let mut responses = 0;
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            if let Http2Event::HeadersReceived { end_stream, .. } = event {
                if end_stream {
                    responses += 1;
                    if responses >= stream_count {
                        break;
                    }
                }
            }
        }
        assert_eq!(responses, stream_count);

        client.shutdown().await.ok();
        let _ = server_handle.await;
    }

    /// nghttp2 クライアント <-> http2 サーバー: 連続 RST_STREAM
    #[tokio::test]
    async fn test_nghttp2_client_http2_server_rapid_rst_stream() {
        let tls_config = generate_http2_test_cert();
        let limits = Limits::default();

        let server = Http2Server::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
            limits,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let rst_count = 10;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");
            let mut rst_sent = 0;

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    Http2Event::HeadersReceived {
                        stream_id,
                        end_stream,
                        ..
                    } => {
                        if end_stream {
                            conn.reset_stream(stream_id, Http2ErrorCode::Cancel)
                                .await
                                .expect("failed to send rst_stream");
                            rst_sent += 1;
                            if rst_sent >= rst_count {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        let mut client = NgClient::connect_insecure(server_addr, "localhost")
            .await
            .expect("failed to connect");

        wait_for_nghttp2_settings_ack(&mut client).await;

        for _ in 0..rst_count {
            let request_headers = vec![
                NgHeader::method("GET"),
                NgHeader::scheme("https"),
                NgHeader::authority("localhost"),
                NgHeader::path("/"),
            ];
            client
                .send_request(&request_headers, None, true)
                .await
                .expect("failed to send request");
        }

        let mut closed = 0;
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            if let NgHttp2Event::StreamClosed { .. } = event {
                closed += 1;
                if closed >= rst_count {
                    break;
                }
            }
        }
        assert_eq!(closed, rst_count);

        client.shutdown().await.ok();
        let _ = server_handle.await;
    }

    /// http2 クライアント <-> nghttp2 サーバー: 連続 RST_STREAM
    #[tokio::test]
    async fn test_http2_client_nghttp2_server_rapid_rst_stream() {
        let tls_config = generate_nghttp2_test_cert();

        let server = NgServer::bind(
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
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    NgHttp2Event::HeadersReceived {
                        stream_id,
                        end_stream,
                        ..
                    } => {
                        if end_stream {
                            conn.reset_stream(stream_id, NgErrorCode::Cancel)
                                .await
                                .expect("failed to send rst_stream");
                            rst_sent += 1;
                            if rst_sent >= rst_count {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        let limits = Limits::default();
        let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
            .await
            .expect("failed to connect");

        wait_for_http2_settings_ack(&mut client).await;

        for _ in 0..rst_count {
            let request_headers = vec![
                HeaderField::new(":method", "GET").expect("valid header field"),
                HeaderField::new(":scheme", "https").expect("valid header field"),
                HeaderField::new(":authority", "localhost").expect("valid header field"),
                HeaderField::new(":path", "/").expect("valid header field"),
            ];
            client
                .send_request(request_headers, true)
                .await
                .expect("failed to send request");
        }

        let mut resets = 0;
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            if let Http2Event::StreamReset { .. } = event {
                resets += 1;
                if resets >= rst_count {
                    break;
                }
            }
        }
        assert_eq!(resets, rst_count);

        client.shutdown().await.ok();
        let _ = server_handle.await;
    }

    /// nghttp2 クライアント <-> http2 サーバー: 大量の小さいデータフレーム
    #[tokio::test]
    async fn test_nghttp2_client_http2_server_many_small_data_frames() {
        let tls_config = generate_http2_test_cert();
        let limits = Limits::default();

        let server = Http2Server::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
            limits,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let chunk_count: usize = 50;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    Http2Event::HeadersReceived {
                        stream_id,
                        end_stream,
                        ..
                    } => {
                        if end_stream {
                            let response_headers = vec![
                                HeaderField::new(":status", "200").expect("valid header field"),
                            ];
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
                    _ => {}
                }
            }
        });

        let mut client = NgClient::connect_insecure(server_addr, "localhost")
            .await
            .expect("failed to connect");

        wait_for_nghttp2_settings_ack(&mut client).await;

        let request_headers = vec![
            NgHeader::method("GET"),
            NgHeader::scheme("https"),
            NgHeader::authority("localhost"),
            NgHeader::path("/"),
        ];
        client
            .send_request(&request_headers, None, true)
            .await
            .expect("failed to send request");

        let mut received_data = Vec::new();
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            match event {
                NgHttp2Event::DataReceived {
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
        assert_eq!(received_data.len(), chunk_count);

        client.shutdown().await.ok();
        let _ = server_handle.await;
    }

    /// http2 クライアント <-> nghttp2 サーバー: 大量の小さいデータフレーム
    #[tokio::test]
    async fn test_http2_client_nghttp2_server_many_small_data_frames() {
        let tls_config = generate_nghttp2_test_cert();

        let server = NgServer::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let chunk_count: usize = 50;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    NgHttp2Event::HeadersReceived {
                        stream_id,
                        end_stream,
                        ..
                    } => {
                        if end_stream {
                            let response_headers = vec![NgHeader::status(200)];
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
                    _ => {}
                }
            }
        });

        let limits = Limits::default();
        let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
            .await
            .expect("failed to connect");

        wait_for_http2_settings_ack(&mut client).await;

        let request_headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":authority", "localhost").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
        ];
        client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");

        let mut received_data = Vec::new();
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            match event {
                Http2Event::DataReceived {
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
        assert_eq!(received_data.len(), chunk_count);

        client.shutdown().await.ok();
        let _ = server_handle.await;
    }

    /// nghttp2 クライアント <-> http2 サーバー: 双方向ストリーミング
    #[tokio::test]
    async fn test_nghttp2_client_http2_server_bidirectional_echo() {
        let tls_config = generate_http2_test_cert();
        let limits = Limits::default();

        let server = Http2Server::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
            limits,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");
            // ヘッダー受信後に設定される
            let mut stream_id: Option<tokio_http2::StreamId> = None;

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    Http2Event::HeadersReceived { stream_id: sid, .. } => {
                        stream_id = Some(sid);
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(sid, response_headers, false)
                            .await
                            .expect("failed to send response");
                    }
                    Http2Event::DataReceived {
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
                    _ => {}
                }
            }
        });

        let mut client = NgClient::connect_insecure(server_addr, "localhost")
            .await
            .expect("failed to connect");

        wait_for_nghttp2_settings_ack(&mut client).await;

        let request_headers = vec![
            NgHeader::method("POST"),
            NgHeader::scheme("https"),
            NgHeader::authority("localhost"),
            NgHeader::path("/echo"),
        ];
        let stream_id = client
            .send_request(&request_headers, None, false)
            .await
            .expect("failed to send request");

        client
            .send_data(stream_id, b"hello-", false)
            .await
            .expect("failed to send data 1");
        client
            .send_data(stream_id, b"world", true)
            .await
            .expect("failed to send data 2");

        let mut received_data = Vec::new();
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            match event {
                NgHttp2Event::DataReceived {
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
        assert_eq!(received_data, b"hello-world");

        client.shutdown().await.ok();
        let _ = server_handle.await;
    }

    /// http2 クライアント <-> nghttp2 サーバー: 双方向ストリーミング
    #[tokio::test]
    async fn test_http2_client_nghttp2_server_bidirectional_echo() {
        let tls_config = generate_nghttp2_test_cert();

        let server = NgServer::bind(
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
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    NgHttp2Event::HeadersReceived { stream_id: sid, .. } => {
                        stream_id = sid;
                        let response_headers = vec![NgHeader::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");
                    }
                    NgHttp2Event::DataReceived {
                        data, end_stream, ..
                    } => {
                        conn.send_data(stream_id, &data, end_stream)
                            .await
                            .expect("failed to echo data");
                        if end_stream {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });

        let limits = Limits::default();
        let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
            .await
            .expect("failed to connect");

        wait_for_http2_settings_ack(&mut client).await;

        let request_headers = vec![
            HeaderField::new(":method", "POST").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":authority", "localhost").expect("valid header field"),
            HeaderField::new(":path", "/echo").expect("valid header field"),
        ];
        let stream_id = client
            .send_request(request_headers, false)
            .await
            .expect("failed to send request");

        client
            .send_data(stream_id, b"hello-".to_vec(), false)
            .await
            .expect("failed to send data 1");
        client
            .send_data(stream_id, b"world".to_vec(), true)
            .await
            .expect("failed to send data 2");

        let mut received_data = Vec::new();
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            match event {
                Http2Event::DataReceived {
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
        assert_eq!(received_data, b"hello-world");

        client.shutdown().await.ok();
        let _ = server_handle.await;
    }

    /// nghttp2 クライアント <-> http2 サーバー: 連続 PING
    #[tokio::test]
    async fn test_nghttp2_client_http2_server_rapid_ping() {
        let tls_config = generate_http2_test_cert();
        let limits = Limits::default();

        let server = Http2Server::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
            limits,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let ping_count: u8 = 10;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    Http2Event::PingReceived { ack, .. } => {
                        if !ack {
                            conn.flush().await.expect("failed to flush");
                        }
                    }
                    _ => {}
                }
            }
        });

        let mut client = NgClient::connect_insecure(server_addr, "localhost")
            .await
            .expect("failed to connect");

        wait_for_nghttp2_settings_ack(&mut client).await;

        for i in 0..ping_count {
            let data = [i, 0, 0, 0, 0, 0, 0, 0];
            client.ping(&data).await.expect("failed to send ping");
        }

        let mut pong_count: u8 = 0;
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
            if let NgHttp2Event::PingReceived { ack: true, .. } = event {
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

    /// http2 クライアント <-> nghttp2 サーバー: 連続 PING
    #[tokio::test]
    async fn test_http2_client_nghttp2_server_rapid_ping() {
        let tls_config = generate_nghttp2_test_cert();

        let server = NgServer::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let ping_count: u8 = 10;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    NgHttp2Event::PingReceived { ack, .. } => {
                        if !ack {
                            conn.flush().await.expect("failed to flush");
                        }
                    }
                    _ => {}
                }
            }
        });

        let limits = Limits::default();
        let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
            .await
            .expect("failed to connect");

        wait_for_http2_settings_ack(&mut client).await;

        for i in 0..ping_count {
            let data = [i, 0, 0, 0, 0, 0, 0, 0];
            client.ping(data).await.expect("failed to send ping");
        }

        let mut pong_count: u8 = 0;
        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => break,
                };
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

    /// nghttp2 クライアント <-> http2 サーバー: 大量ヘッダー
    #[tokio::test]
    async fn test_nghttp2_client_http2_server_many_headers_stress() {
        let tls_config = generate_http2_test_cert();
        let limits = Limits::default();

        let server = Http2Server::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
            limits,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let header_count = 40;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    Http2Event::HeadersReceived {
                        stream_id,
                        headers,
                        end_stream,
                        ..
                    } => {
                        if end_stream {
                            let custom_count = headers
                                .iter()
                                .filter(|h| h.name().starts_with(b"x-stress-"))
                                .count();
                            assert_eq!(custom_count, header_count);

                            let response_headers = vec![
                                HeaderField::new(":status", "200").expect("valid header field"),
                            ];
                            conn.send_response(stream_id, response_headers, true)
                                .await
                                .expect("failed to send response");
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });

        let mut client = NgClient::connect_insecure(server_addr, "localhost")
            .await
            .expect("failed to connect");

        wait_for_nghttp2_settings_ack(&mut client).await;

        let mut request_headers = vec![
            NgHeader::method("GET"),
            NgHeader::scheme("https"),
            NgHeader::authority("localhost"),
            NgHeader::path("/"),
        ];
        for i in 0..header_count {
            request_headers.push(NgHeader::new(
                format!("x-stress-{}", i).into_bytes(),
                format!("value-{}", i).into_bytes(),
            ));
        }

        let stream_id = client
            .send_request(&request_headers, None, true)
            .await
            .expect("failed to send request");

        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => panic!("timeout waiting for response"),
                };
            if let NgHttp2Event::HeadersReceived {
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
        let _ = server_handle.await;
    }

    /// http2 クライアント <-> nghttp2 サーバー: 大量ヘッダー
    #[tokio::test]
    async fn test_http2_client_nghttp2_server_many_headers_stress() {
        let tls_config = generate_nghttp2_test_cert();

        let server = NgServer::bind(
            "127.0.0.1:0".parse().expect("parse should succeed"),
            tls_config,
        )
        .await
        .expect("failed to bind server");
        let server_addr = server.local_addr();

        let header_count = 40;

        let server_handle = tokio::spawn(async move {
            let mut conn = server.accept().await.expect("failed to accept connection");

            loop {
                let event =
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(e)) => e,
                        _ => break,
                    };
                match event {
                    NgHttp2Event::HeadersReceived {
                        stream_id,
                        headers,
                        end_stream,
                        ..
                    } => {
                        if end_stream {
                            let custom_count = headers
                                .iter()
                                .filter(|h| h.name.starts_with(b"x-stress-"))
                                .count();
                            assert_eq!(custom_count, header_count);

                            let response_headers = vec![NgHeader::status(200)];
                            conn.send_response(stream_id, &response_headers, true)
                                .await
                                .expect("failed to send response");
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });

        let limits = Limits::default();
        let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
            .await
            .expect("failed to connect");

        wait_for_http2_settings_ack(&mut client).await;

        let mut request_headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":authority", "localhost").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
        ];
        for i in 0..header_count {
            request_headers.push(
                HeaderField::new(format!("x-stress-{}", i), format!("value-{}", i))
                    .expect("valid header field"),
            );
        }

        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("failed to send request");

        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => panic!("timeout waiting for response"),
                };
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
        let _ = server_handle.await;
    }
}

// ============================================================================
// トレーラーテスト (RFC 9113 Section 8.1)
// ============================================================================

/// nghttp2 サーバーがトレーラーを送信 → http2 クライアントが TrailersReceived を受信
#[tokio::test]
async fn test_http2_client_nghttp2_server_trailers() {
    let tls_config = generate_nghttp2_test_cert();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![NgHeader::new(b":status".to_vec(), b"200".to_vec())];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");

                        conn.send_data(stream_id, b"hello", false)
                            .await
                            .expect("failed to send data");
                        conn.flush().await.expect("failed to flush");

                        let trailer_headers =
                            vec![NgHeader::new(b"x-trailer".to_vec(), b"value".to_vec())];
                        conn.send_trailer(stream_id, &trailer_headers)
                            .await
                            .expect("failed to send trailer");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let limits = Limits::default();
    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_data = false;
    let mut received_trailers = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            Http2Event::HeadersReceived { .. } => {}
            Http2Event::DataReceived {
                data, end_stream, ..
            } => {
                assert!(
                    !end_stream,
                    "data should not have end_stream before trailers"
                );
                assert_eq!(data, b"hello");
                received_data = true;
            }
            Http2Event::TrailersReceived { trailers, .. } => {
                let trailer = trailers
                    .iter()
                    .find(|h| h.name() == b"x-trailer")
                    .expect("missing x-trailer");
                assert_eq!(trailer.value(), b"value");
                received_trailers = true;
                break;
            }
            Http2Event::SettingsReceived { .. }
            | Http2Event::ConnectionPreface
            | Http2Event::WindowUpdateReceived { .. } => {}
            _ => {}
        }
    }

    assert!(received_data);
    assert!(received_trailers);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 サーバーがトレーラーを送信 → nghttp2 クライアントが受信
#[tokio::test]
async fn test_nghttp2_client_http2_server_trailers() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, false)
                            .await
                            .expect("failed to send response");

                        conn.send_data(stream_id, vec![1, 2, 3], false)
                            .await
                            .expect("failed to send data");

                        let trailer_headers =
                            vec![HeaderField::new("x-result", "ok").expect("valid header field")];
                        conn.send_trailers(stream_id, trailer_headers)
                            .await
                            .expect("failed to send trailers");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");

    let mut received_data = false;
    let mut received_trailers = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            NgHttp2Event::HeadersReceived {
                headers,
                end_stream,
                ..
            } => {
                // nghttp2 ではトレーラーは疑似ヘッダー :status を含まない終了ヘッダーとして通知される
                if end_stream && !headers.iter().any(|h| h.name == b":status") {
                    let trailer = headers
                        .iter()
                        .find(|h| h.name == b"x-result")
                        .expect("missing x-result trailer");
                    assert_eq!(trailer.value, b"ok");
                    received_trailers = true;
                    break;
                }
            }
            NgHttp2Event::DataReceived {
                data, end_stream, ..
            } => {
                assert!(!end_stream);
                assert_eq!(data, vec![1, 2, 3]);
                received_data = true;
            }
            NgHttp2Event::SettingsReceived { .. } => {}
            _ => {}
        }
    }

    assert!(received_data);
    assert!(received_trailers);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアントがトレーラーを送信 → nghttp2 サーバーが受信
#[tokio::test]
async fn test_http2_client_nghttp2_server_client_sends_trailers() {
    let tls_config = generate_nghttp2_test_cert();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut received_data = false;
        let mut received_trailers = false;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                })) => {
                    let is_request = headers.iter().any(|h| h.name == b":method");
                    if is_request {
                        let response_headers =
                            vec![NgHeader::new(b":status".to_vec(), b"200".to_vec())];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                    } else if end_stream {
                        let trailer = headers
                            .iter()
                            .find(|h| h.name == b"x-client-trailer")
                            .expect("missing x-client-trailer");
                        assert_eq!(trailer.value, b"end");
                        received_trailers = true;
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::DataReceived { end_stream, .. })) => {
                    if !end_stream {
                        received_data = true;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(received_data);
        assert!(received_trailers);
    });

    let limits = Limits::default();
    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "POST").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request");

    client
        .send_data(stream_id, vec![1, 2, 3], false)
        .await
        .expect("failed to send data");

    let trailer_headers =
        vec![HeaderField::new("x-client-trailer", "end").expect("valid header field")];
    client
        .send_trailers(stream_id, trailer_headers)
        .await
        .expect("failed to send trailers");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::HeadersReceived { end_stream, .. } = event
            && end_stream
        {
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 クライアントがトレーラーを送信 → http2 サーバーが TrailersReceived を受信
#[tokio::test]
async fn test_nghttp2_client_http2_server_client_sends_trailers() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut received_data = false;
        let mut received_trailers = false;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id: sid,
                    end_stream,
                    ..
                })) => {
                    if !end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(sid, response_headers, true)
                            .await
                            .expect("failed to send response");
                    }
                }
                Ok(Ok(Http2Event::DataReceived { end_stream, .. })) => {
                    if !end_stream {
                        received_data = true;
                    }
                }
                Ok(Ok(Http2Event::TrailersReceived { trailers, .. })) => {
                    let trailer = trailers
                        .iter()
                        .find(|h| h.name() == b"x-ng-trailer")
                        .expect("missing x-ng-trailer");
                    assert_eq!(trailer.value(), b"done");
                    received_trailers = true;
                    break;
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(received_data);
        assert!(received_trailers);
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("POST"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, Some(b"hello"), false)
        .await
        .expect("failed to send request");

    let trailer_headers = vec![NgHeader::new(b"x-ng-trailer".to_vec(), b"done".to_vec())];
    client
        .send_trailer(stream_id, &trailer_headers)
        .await
        .expect("failed to send trailer");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::HeadersReceived { end_stream, .. } = event
            && end_stream
        {
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 1xx 情報レスポンステスト (RFC 9113 Section 8.1 / Section 8.8.5)
// ============================================================================

/// http2 サーバーが 100 Continue → 200 OK を送信 → nghttp2 クライアントが受信
#[tokio::test]
async fn test_nghttp2_client_http2_server_1xx_informational() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let informational_headers =
                            vec![HeaderField::new(":status", "100").expect("valid header field")];
                        conn.send_response(stream_id, informational_headers, false)
                            .await
                            .expect("failed to send 100 Continue");

                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send 200 OK");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");

    let mut saw_100 = false;
    let mut saw_200 = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::HeadersReceived {
            headers,
            end_stream,
            ..
        } = event
        {
            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            if status.value == b"100" {
                saw_100 = true;
            } else if status.value == b"200" {
                assert!(end_stream);
                saw_200 = true;
                break;
            }
        }
    }

    assert!(saw_100, "100 Continue を受信すべき");
    assert!(saw_200, "200 OK を受信すべき");

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 サーバーが 103 Early Hints → 200 OK を送信 → http2 クライアントが受信
#[tokio::test]
async fn test_http2_client_nghttp2_server_1xx_informational() {
    let tls_config = generate_nghttp2_test_cert();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let informational_headers =
                            vec![NgHeader::new(b":status".to_vec(), b"103".to_vec())];
                        conn.send_headers(stream_id, &informational_headers, false)
                            .await
                            .expect("failed to send 103 Early Hints");

                        let response_headers =
                            vec![NgHeader::new(b":status".to_vec(), b"200".to_vec())];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send 200 OK");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let limits = Limits::default();
    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut saw_103 = false;
    let mut saw_200 = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::HeadersReceived {
            headers,
            end_stream,
            ..
        } = event
        {
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            if status.value() == b"103" {
                saw_103 = true;
            } else if status.value() == b"200" {
                assert!(end_stream);
                saw_200 = true;
                break;
            }
        }
    }

    assert!(saw_103, "103 Early Hints を受信すべき");
    assert!(saw_200, "200 OK を受信すべき");

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// Cookie 結合テスト (RFC 9113 Section 8.2.3)
// ============================================================================

/// nghttp2 クライアントが複数 Cookie ヘッダーを送信 → http2 サーバーが結合された Cookie を受信
#[tokio::test]
async fn test_nghttp2_client_http2_server_cookie_concatenation() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        // Cookie はセミコロン区切りで 1 つに結合される (RFC 9113 §8.2.3)
                        let cookie = headers
                            .iter()
                            .find(|h| h.name() == b"cookie")
                            .expect("missing cookie header");
                        assert_eq!(cookie.value(), b"a=1; b=2; c=3");

                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let mut request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    request_headers.push(NgHeader::new(b"cookie".to_vec(), b"a=1".to_vec()));
    request_headers.push(NgHeader::new(b"cookie".to_vec(), b"b=2".to_vec()));
    request_headers.push(NgHeader::new(b"cookie".to_vec(), b"c=3".to_vec()));

    client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::HeadersReceived {
            headers,
            end_stream,
            ..
        } = event
            && end_stream
        {
            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアントが複数 Cookie ヘッダーを送信 → nghttp2 サーバーが受信
#[tokio::test]
async fn test_http2_client_nghttp2_server_cookie_concatenation() {
    let tls_config = generate_nghttp2_test_cert();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let cookie = headers
                            .iter()
                            .find(|h| h.name == b"cookie")
                            .expect("missing cookie header");
                        assert_eq!(cookie.value, b"x=foo; y=bar");

                        let response_headers =
                            vec![NgHeader::new(b":status".to_vec(), b"200".to_vec())];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let limits = Limits::default();
    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new("cookie", "x=foo").expect("valid header field"),
        HeaderField::new("cookie", "y=bar").expect("valid header field"),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::HeadersReceived {
            headers,
            end_stream,
            ..
        } = event
            && end_stream
        {
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// GOAWAY 後のストリーム完了テスト (RFC 9113 Section 6.8)
// ============================================================================

/// http2 サーバーが GOAWAY で正常終了 → nghttp2 クライアントが GoawayReceived を受信
/// 接続内でストリームが完了した後に GOAWAY を送信するパターン
#[tokio::test]
async fn test_http2_client_nghttp2_server_goaway_graceful() {
    let tls_config = generate_nghttp2_test_cert();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![NgHeader::new(b":status".to_vec(), b"200".to_vec())];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");

                        conn.shutdown(stream_id)
                            .await
                            .expect("failed to send goaway");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let limits = Limits::default();
    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut got_response = false;
    let mut got_goaway = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            Http2Event::HeadersReceived { end_stream, .. } => {
                if end_stream {
                    got_response = true;
                }
            }
            Http2Event::GoawayReceived { .. } => {
                got_goaway = true;
                break;
            }
            _ => {}
        }
    }

    assert!(got_response);
    assert!(got_goaway);

    let _ = server_handle.await;
}

// ============================================================================
// フロー制御 WINDOW_UPDATE テスト (RFC 9113 Section 6.9)
// ============================================================================

/// http2 クライアントが WINDOW_UPDATE を手動送信 → nghttp2 サーバーがデータを継続送信
#[tokio::test]
async fn test_http2_client_nghttp2_server_manual_window_update() {
    let tls_config = generate_nghttp2_test_cert();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![NgHeader::new(b":status".to_vec(), b"200".to_vec())];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");

                        let chunk1: Vec<u8> = vec![0xAA; 8192];
                        let chunk2: Vec<u8> = vec![0xBB; 8192];
                        conn.send_data(stream_id, &chunk1, false)
                            .await
                            .expect("failed to send data chunk 1");
                        conn.flush().await.expect("failed to flush");
                        conn.send_data(stream_id, &chunk2, true)
                            .await
                            .expect("failed to send data chunk 2");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    let limits = Limits::default();
    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_data = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::DataReceived {
            data, end_stream, ..
        } = event
        {
            received_data.extend_from_slice(&data);
            if end_stream {
                break;
            }
            client
                .send_window_update(stream_id, 8192)
                .await
                .expect("failed to send window_update");
        }
    }

    assert_eq!(received_data.len(), 16384);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 不足方向の HTTP メソッドテスト
// ============================================================================

/// http2 クライアント <-> nghttp2 サーバー: PUT メソッド
#[tokio::test]
async fn test_http2_client_nghttp2_server_put() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value, b"PUT");

                        let response_headers = vec![NgHeader::status(204)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "PUT").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/resource").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"204");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 クライアント <-> http2 サーバー: DELETE メソッド
#[tokio::test]
async fn test_nghttp2_client_http2_server_delete() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name() == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value(), b"DELETE");

                        let response_headers =
                            vec![HeaderField::new(":status", "204").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("DELETE"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/resource/123"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"204");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: HEAD メソッド
#[tokio::test]
async fn test_http2_client_nghttp2_server_head() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name == b":method")
                            .expect("missing :method header");
                        assert_eq!(method.value, b"HEAD");

                        let response_headers = vec![
                            NgHeader::status(200),
                            NgHeader::new(b"content-length".to_vec(), b"1234".to_vec()),
                        ];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "HEAD").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);
            assert!(end_stream);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");

            let content_length = headers
                .iter()
                .find(|h| h.name() == b"content-length")
                .expect("missing content-length header");
            assert_eq!(content_length.value(), b"1234");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 不足方向のステータスコードテスト
// ============================================================================

/// http2 クライアント <-> nghttp2 サーバー: 404 Not Found
#[tokio::test]
async fn test_http2_client_nghttp2_server_404() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![NgHeader::status(404)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/not-found").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"404");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 クライアント <-> http2 サーバー: 500 Internal Server Error
#[tokio::test]
async fn test_nghttp2_client_http2_server_500() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "500").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/error"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"500");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 404 レスポンスにボディ付き
#[tokio::test]
async fn test_http2_client_nghttp2_server_404_with_body() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let error_body = b"Not Found: the requested resource does not exist";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![
                            NgHeader::status(404),
                            NgHeader::new(b"content-type".to_vec(), b"text/plain".to_vec()),
                        ];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        conn.send_data(stream_id, error_body, true)
                            .await
                            .expect("failed to send error body");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/not-found").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_status = Vec::new();
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                received_status = status.value().to_vec();
            }
            Http2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::SettingsReceived { .. } | Http2Event::ConnectionPreface => {}
            _ => {}
        }
    }

    assert_eq!(received_status, b"404");
    assert_eq!(received_body, error_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 500 レスポンスにボディ付き
#[tokio::test]
async fn test_http2_client_nghttp2_server_500_with_body() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let error_body = b"Internal Server Error: an unexpected error occurred";

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![
                            NgHeader::status(500),
                            NgHeader::new(b"content-type".to_vec(), b"text/plain".to_vec()),
                        ];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        conn.send_data(stream_id, error_body, true)
                            .await
                            .expect("failed to send error body");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/error").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_status = Vec::new();
    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        match event {
            Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                headers,
                ..
            } => {
                assert_eq!(recv_stream_id, stream_id);
                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("missing :status header");
                received_status = status.value().to_vec();
            }
            Http2Event::DataReceived {
                data, end_stream, ..
            } => {
                received_body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::SettingsReceived { .. } | Http2Event::ConnectionPreface => {}
            _ => {}
        }
    }

    assert_eq!(received_status, b"500");
    assert_eq!(received_body, error_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 不足方向の RST_STREAM エラーコードテスト
// ============================================================================

/// http2 クライアント <-> nghttp2 サーバー: RST_STREAM InternalError
#[tokio::test]
async fn test_http2_client_nghttp2_server_rst_stream_internal_error() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        conn.reset_stream(stream_id, NgErrorCode::InternalError)
                            .await
                            .expect("failed to send rst_stream");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::StreamReset {
            stream_id: reset_stream_id,
            error_code,
        } = event
        {
            assert_eq!(reset_stream_id, stream_id);
            assert_eq!(error_code, Http2ErrorCode::InternalError);
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 クライアント <-> http2 サーバー: RST_STREAM RefusedStream
#[tokio::test]
async fn test_nghttp2_client_http2_server_rst_stream_refused() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        conn.reset_stream(stream_id, Http2ErrorCode::RefusedStream)
                            .await
                            .expect("failed to send rst_stream");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::StreamClosed {
            stream_id: closed_stream_id,
            error_code,
        } = event
        {
            assert_eq!(closed_stream_id, stream_id);
            assert_eq!(error_code, NgErrorCode::RefusedStream);
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 不足方向の大規模データ転送テスト
// ============================================================================

/// http2 クライアント <-> nghttp2 サーバー: 中程度のレスポンスボディ
#[tokio::test]
async fn test_http2_client_nghttp2_server_large_response() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    // 16KB のレスポンスボディ（初期ウィンドウサイズ内）
    let response_body: Vec<u8> = (0..16384).map(|i| (i % 256) as u8).collect();
    let response_body_clone = response_body.clone();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![NgHeader::status(200)];
                        conn.send_response(stream_id, &response_headers, false)
                            .await
                            .expect("failed to send response headers");

                        conn.send_data(stream_id, &response_body_clone, true)
                            .await
                            .expect("failed to send response body");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/large").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut received_body = Vec::new();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(10), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            Http2Event::HeadersReceived { .. } => {}
            Http2Event::DataReceived {
                stream_id: recv_stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(recv_stream_id, stream_id);
                received_body.extend_from_slice(&data);

                if end_stream {
                    break;
                }
            }
            Http2Event::SettingsReceived { .. }
            | Http2Event::ConnectionPreface
            | Http2Event::WindowUpdateReceived { .. } => {}
            _ => {}
        }
    }

    assert_eq!(received_body.len(), response_body.len());
    assert_eq!(received_body, response_body);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 クライアント <-> http2 サーバー: 中程度のリクエストボディ
#[tokio::test]
async fn test_nghttp2_client_http2_server_large_request() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    // 16KB のリクエストボディ（初期ウィンドウサイズ内）
    let request_body: Vec<u8> = (0..16384).map(|i| (i % 256) as u8).collect();
    let request_body_clone = request_body.clone();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut received_body = Vec::new();
        let mut request_stream_id: Option<tokio_http2::StreamId> = None;

        loop {
            match tokio::time::timeout(Duration::from_secs(10), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    request_stream_id = Some(stream_id);
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(Http2Event::DataReceived {
                    data, end_stream, ..
                })) => {
                    received_body.extend_from_slice(&data);
                    if end_stream {
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert_eq!(received_body.len(), request_body_clone.len());
        assert_eq!(received_body, request_body_clone);

        let sid = request_stream_id.expect("headers not received");
        let response_headers =
            vec![HeaderField::new(":status", "200").expect("valid header field")];
        conn.send_response(sid, response_headers, true)
            .await
            .expect("failed to send response");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("POST"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/upload"),
    ];
    let stream_id = client
        .send_request(&request_headers, Some(&request_body), true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(10), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 複数 http2 クライアント → nghttp2 サーバーテスト
// ============================================================================

/// http2 クライアント複数 <-> nghttp2 サーバー: 同時接続
#[tokio::test]
async fn test_multiple_http2_clients_nghttp2_server() {
    let tls_config = generate_nghttp2_test_cert();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        for _ in 0..2 {
            let mut conn = server.accept().await.expect("failed to accept connection");

            tokio::spawn(async move {
                loop {
                    match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                        Ok(Ok(NgHttp2Event::HeadersReceived {
                            stream_id,
                            end_stream,
                            ..
                        })) => {
                            if end_stream {
                                let response_headers = vec![NgHeader::status(200)];
                                conn.send_response(stream_id, &response_headers, true)
                                    .await
                                    .expect("failed to send response");
                                conn.flush().await.expect("failed to flush");
                            }
                        }
                        Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                        Ok(Ok(_)) => {}
                        _ => break,
                    }
                }
            });
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let limits = Limits::default();
    let limits2 = Limits::default();

    let client1_handle = tokio::spawn(async move {
        let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
            .await
            .expect("client1: failed to connect");

        wait_for_http2_settings_ack(&mut client).await;

        let request_headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":authority", "localhost").expect("valid header field"),
            HeaderField::new(":path", "/client1").expect("valid header field"),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("client1: failed to send request");

        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => panic!("client1: timeout"),
                };
            if let Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                ..
            } = event
            {
                assert_eq!(recv_stream_id, stream_id);
                break;
            }
        }
        client.shutdown().await.ok();
    });

    let client2_handle = tokio::spawn(async move {
        let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits2)
            .await
            .expect("client2: failed to connect");

        wait_for_http2_settings_ack(&mut client).await;

        let request_headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":authority", "localhost").expect("valid header field"),
            HeaderField::new(":path", "/client2").expect("valid header field"),
        ];
        let stream_id = client
            .send_request(request_headers, true)
            .await
            .expect("client2: failed to send request");

        loop {
            let event =
                match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
                    Ok(Ok(e)) => e,
                    _ => panic!("client2: timeout"),
                };
            if let Http2Event::HeadersReceived {
                stream_id: recv_stream_id,
                ..
            } = event
            {
                assert_eq!(recv_stream_id, stream_id);
                break;
            }
        }
        client.shutdown().await.ok();
    });

    let (r1, r2) = tokio::join!(client1_handle, client2_handle);
    r1.expect("client1 failed");
    r2.expect("client2 failed");

    server_handle.abort();
}

// ============================================================================
// GOAWAY エラーコード・デバッグデータテスト (RFC 9113 Section 6.8)
// ============================================================================

/// nghttp2 サーバーが GOAWAY を ProtocolError で送信 → http2 クライアントが受信
#[tokio::test]
async fn test_http2_client_nghttp2_server_goaway_protocol_error() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::SettingsReceived { ack: true })) => break,
                Ok(Ok(_)) => {}
                _ => return,
            }
        }

        conn.terminate(NgErrorCode::ProtocolError)
            .await
            .expect("failed to send goaway");
        conn.flush().await.expect("failed to flush");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::GoawayReceived { error_code, .. } = event {
            assert_eq!(error_code, Http2ErrorCode::ProtocolError);
            break;
        }
    }

    let _ = server_handle.await;
}

/// nghttp2 クライアントが GOAWAY を InternalError で送信 → http2 サーバーが受信
#[tokio::test]
async fn test_nghttp2_client_http2_server_goaway_internal_error() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::GoawayReceived { error_code, .. })) => {
                    assert_eq!(error_code, Http2ErrorCode::InternalError);
                    break;
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    client
        .terminate(NgErrorCode::InternalError)
        .await
        .expect("failed to send goaway");

    let _ = server_handle.await;
}

/// nghttp2 サーバーがストリーム完了後に GOAWAY を送信 → http2 クライアントが両方受信
#[tokio::test]
async fn test_http2_client_nghttp2_server_goaway_after_response() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![NgHeader::status(200)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");

                        conn.shutdown(stream_id)
                            .await
                            .expect("failed to send goaway");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
    ];
    client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    let mut got_response = false;
    let mut got_goaway = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        match event {
            Http2Event::HeadersReceived { end_stream, .. } => {
                if end_stream {
                    got_response = true;
                }
            }
            Http2Event::GoawayReceived {
                last_stream_id,
                error_code,
                ..
            } => {
                assert!(last_stream_id.as_u32() >= 1);
                assert_eq!(error_code, Http2ErrorCode::NoError);
                got_goaway = true;
                break;
            }
            _ => {}
        }
    }

    assert!(got_response);
    assert!(got_goaway);

    let _ = server_handle.await;
}

// ============================================================================
// 混在メソッドテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: GET と POST を同一接続で混在
#[tokio::test]
async fn test_nghttp2_client_http2_server_mixed_methods() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut streams_responded = 0;
        let mut post_stream_id: Option<tokio_http2::StreamId> = None;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let method = headers
                            .iter()
                            .find(|h| h.name() == b":method")
                            .expect("missing :method");
                        let status = match method.value() {
                            b"GET" => b"200",
                            b"POST" => b"201",
                            _ => b"200",
                        };

                        let response_headers =
                            vec![HeaderField::new(":status", status).expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");

                        streams_responded += 1;
                        if streams_responded >= 2 {
                            break;
                        }
                    } else {
                        post_stream_id = Some(stream_id);
                    }
                }
                Ok(Ok(Http2Event::DataReceived { end_stream, .. })) => {
                    if end_stream {
                        let sid = post_stream_id.expect("POST stream ID not set");
                        let response_headers =
                            vec![HeaderField::new(":status", "201").expect("valid header field")];
                        conn.send_response(sid, response_headers, true)
                            .await
                            .expect("failed to send response");

                        streams_responded += 1;
                        if streams_responded >= 2 {
                            break;
                        }
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    // GET リクエスト
    let get_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/"),
    ];
    let get_stream_id = client
        .send_request(&get_headers, None, true)
        .await
        .expect("failed to send GET");
    client.flush().await.expect("failed to flush");

    // POST リクエスト (ボディ付き)
    let post_headers = vec![
        NgHeader::method("POST"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/create"),
    ];
    let post_stream_id = client
        .send_request(&post_headers, Some(b"data"), true)
        .await
        .expect("failed to send POST");
    client.flush().await.expect("failed to flush");

    let mut get_response = false;
    let mut post_response = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            if stream_id == get_stream_id {
                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status");
                assert_eq!(status.value, b"200");
                assert!(end_stream);
                get_response = true;
            }
            if stream_id == post_stream_id {
                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("missing :status");
                assert_eq!(status.value, b"201");
                assert!(end_stream);
                post_response = true;
            }
        }

        if get_response && post_response {
            break;
        }
    }

    assert!(get_response);
    assert!(post_response);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: GET と POST を同一接続で混在
#[tokio::test]
async fn test_http2_client_nghttp2_server_mixed_methods() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut streams_responded = 0;
        let mut request_stream_id = 0;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                })) => {
                    let method = headers
                        .iter()
                        .find(|h| h.name == b":method")
                        .expect("missing :method");
                    let status = match method.value.as_slice() {
                        b"GET" => b"200",
                        b"POST" => {
                            request_stream_id = stream_id;
                            if end_stream {
                                b"201"
                            } else {
                                continue;
                            }
                        }
                        _ => b"200",
                    };

                    let response_headers = vec![NgHeader::status(
                        std::str::from_utf8(status)
                            .expect("should succeed")
                            .parse()
                            .expect("parse should succeed"),
                    )];
                    conn.send_response(stream_id, &response_headers, true)
                        .await
                        .expect("failed to send response");
                    conn.flush().await.expect("failed to flush");

                    if method.value != b"POST" || end_stream {
                        streams_responded += 1;
                    }
                    if streams_responded >= 2 {
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::DataReceived { end_stream, .. })) => {
                    if end_stream {
                        let response_headers = vec![NgHeader::status(201)];
                        conn.send_response(request_stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        streams_responded += 1;
                        if streams_responded >= 2 {
                            break;
                        }
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    // GET リクエスト
    let get_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
    ];
    let get_stream_id = client
        .send_request(get_headers, true)
        .await
        .expect("failed to send GET");

    // POST リクエスト (ボディ付き)
    let post_headers = vec![
        HeaderField::new(":method", "POST").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":path", "/create").expect("valid header field"),
    ];
    let post_stream_id = client
        .send_request(post_headers, false)
        .await
        .expect("failed to send POST headers");
    client
        .send_data(post_stream_id, b"data".to_vec(), true)
        .await
        .expect("failed to send POST body");

    let mut get_response = false;
    let mut post_response = false;

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => break,
        };
        if let Http2Event::HeadersReceived {
            stream_id,
            headers,
            end_stream,
            ..
        } = event
        {
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status");

            if stream_id == get_stream_id {
                assert_eq!(status.value(), b"200");
                assert!(end_stream);
                get_response = true;
            }
            if stream_id == post_stream_id {
                assert_eq!(status.value(), b"201");
                assert!(end_stream);
                post_response = true;
            }
        }

        if get_response && post_response {
            break;
        }
    }

    assert!(get_response);
    assert!(post_response);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// PATCH メソッドテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: PATCH メソッド
#[tokio::test]
async fn test_nghttp2_client_http2_server_patch() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut patch_stream_id: Option<tokio_http2::StreamId> = None;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    let method = headers
                        .iter()
                        .find(|h| h.name() == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value(), b"PATCH");

                    if end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    } else {
                        patch_stream_id = Some(stream_id);
                    }
                }
                Ok(Ok(Http2Event::DataReceived { end_stream, .. })) => {
                    if end_stream {
                        let sid = patch_stream_id.expect("PATCH stream ID not set");
                        let response_headers =
                            vec![HeaderField::new(":status", "200").expect("valid header field")];
                        conn.send_response(sid, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("PATCH"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/resource"),
    ];
    let stream_id = client
        .send_request(&request_headers, Some(b"patch data"), true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: PATCH メソッド
#[tokio::test]
async fn test_http2_client_nghttp2_server_patch() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut request_stream_id = 0;
        let mut received_body = Vec::new();
        let mut body_done = false;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    ..
                })) => {
                    let method = headers
                        .iter()
                        .find(|h| h.name == b":method")
                        .expect("missing :method header");
                    assert_eq!(method.value, b"PATCH");
                    request_stream_id = stream_id;

                    if end_stream {
                        body_done = true;
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::DataReceived {
                    data, end_stream, ..
                })) => {
                    received_body.extend_from_slice(&data);
                    if end_stream {
                        body_done = true;
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(body_done);

        let response_headers = vec![NgHeader::status(200)];
        conn.send_response(request_stream_id, &response_headers, true)
            .await
            .expect("failed to send response");
        conn.flush().await.expect("failed to flush");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "PATCH").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/resource").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, false)
        .await
        .expect("failed to send request headers");

    client
        .send_data(stream_id, b"patch data".to_vec(), true)
        .await
        .expect("failed to send request body");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"200");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// 追加 HTTP ステータスコードテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: 301 Moved Permanently
#[tokio::test]
async fn test_nghttp2_client_http2_server_301() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![
                            HeaderField::new(":status", "301").expect("valid header field"),
                            HeaderField::new("location", "/new-location")
                                .expect("valid header field"),
                        ];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/old-path"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"301");

            let location = headers
                .iter()
                .find(|h| h.name == b"location")
                .expect("missing location header");
            assert_eq!(location.value, b"/new-location");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 302 Found
#[tokio::test]
async fn test_http2_client_nghttp2_server_302() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![
                            NgHeader::status(302),
                            NgHeader::new(b"location".to_vec(), b"/temporary".to_vec()),
                        ];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/old-path").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"302");

            let location = headers
                .iter()
                .find(|h| h.name() == b"location")
                .expect("missing location header");
            assert_eq!(location.value(), b"/temporary");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// nghttp2 クライアント <-> http2 サーバー: 400 Bad Request
#[tokio::test]
async fn test_nghttp2_client_http2_server_400() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers =
                            vec![HeaderField::new(":status", "400").expect("valid header field")];
                        conn.send_response(stream_id, response_headers, true)
                            .await
                            .expect("failed to send response");
                        break;
                    }
                }
                Ok(Ok(Http2Event::SettingsReceived { .. }))
                | Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority("localhost"),
        NgHeader::path("/bad-request"),
    ];
    let stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send request");
    client.flush().await.expect("failed to flush");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let NgHttp2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name == b":status")
                .expect("missing :status header");
            assert_eq!(status.value, b"400");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: 403 Forbidden
#[tokio::test]
async fn test_http2_client_nghttp2_server_403() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::HeadersReceived {
                    stream_id,
                    end_stream,
                    ..
                })) => {
                    if end_stream {
                        let response_headers = vec![NgHeader::status(403)];
                        conn.send_response(stream_id, &response_headers, true)
                            .await
                            .expect("failed to send response");
                        conn.flush().await.expect("failed to flush");
                        break;
                    }
                }
                Ok(Ok(NgHttp2Event::SettingsReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    let request_headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/forbidden").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
    ];
    let stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send request");

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(e)) => e,
            _ => panic!("timeout waiting for response"),
        };
        if let Http2Event::HeadersReceived {
            stream_id: recv_stream_id,
            headers,
            ..
        } = event
        {
            assert_eq!(recv_stream_id, stream_id);

            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("missing :status header");
            assert_eq!(status.value(), b"403");
            break;
        }
    }

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

// ============================================================================
// SETTINGS ネゴシエーションテスト
// ============================================================================

/// nghttp2 クライアント <-> http2 サーバー: SETTINGS の値を相互確認
#[tokio::test]
async fn test_nghttp2_client_http2_server_settings_values() {
    let tls_config = generate_http2_test_cert();
    let limits = Limits::default();

    let server = Http2Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
        limits,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut ack_received = false;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(Http2Event::SettingsReceived { ack })) => {
                    if ack {
                        ack_received = true;
                        break;
                    }
                }
                Ok(Ok(Http2Event::ConnectionPreface))
                | Ok(Ok(Http2Event::WindowUpdateReceived { .. })) => {}
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(ack_received);
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = NgClient::connect_insecure(server_addr, "localhost")
        .await
        .expect("failed to connect");

    wait_for_nghttp2_settings_ack(&mut client).await;

    let remote_initial_window = client.get_remote_settings(SettingsId::InitialWindowSize);
    assert!(remote_initial_window >= 65535);

    let remote_max_streams = client.get_remote_settings(SettingsId::MaxConcurrentStreams);
    assert!(remote_max_streams > 0);

    client.shutdown().await.ok();
    let _ = server_handle.await;
}

/// http2 クライアント <-> nghttp2 サーバー: SETTINGS の値を相互確認
#[tokio::test]
async fn test_http2_client_nghttp2_server_settings_values() {
    let tls_config = generate_nghttp2_test_cert();
    let limits = Limits::default();

    let server = NgServer::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls_config,
    )
    .await
    .expect("failed to bind server");
    let server_addr = server.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("failed to accept connection");
        let mut ack_received = false;

        loop {
            match tokio::time::timeout(Duration::from_secs(5), conn.next_event()).await {
                Ok(Ok(NgHttp2Event::SettingsReceived { ack })) => {
                    if ack {
                        ack_received = true;
                        break;
                    }
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(ack_received);

        let remote_initial_window = conn.get_remote_settings(SettingsId::InitialWindowSize);
        assert!(remote_initial_window >= 65535);

        let local_max_frame = conn.get_local_settings(SettingsId::MaxFrameSize);
        assert!(local_max_frame >= 16384);
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = tokio_http2::Client::connect_insecure(server_addr, "localhost", limits)
        .await
        .expect("failed to connect");

    wait_for_http2_settings_ack(&mut client).await;

    client.shutdown().await.ok();
    let _ = server_handle.await;
}
