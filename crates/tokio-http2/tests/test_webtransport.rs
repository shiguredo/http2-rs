//! WebTransport over HTTP/2 サーバー統合テスト
#![allow(clippy::collapsible_match, clippy::collapsible_if)]

use std::time::Duration;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use shiguredo_http2::settings::WtInitialSettings;
use shiguredo_http2::webtransport::{WtConfig, WtEvent, WtSession};

use tokio_http2::{Client, Event, HeaderField, Limits, Server, TlsServerConfig, WtServerRequest};

fn test_tls() -> TlsServerConfig {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("failed to generate cert");
    TlsServerConfig::from_der(
        vec![CertificateDer::from(cert.der().to_vec())],
        PrivateKeyDer::try_from(signing_key.serialize_der()).unwrap(),
    )
    .expect("failed to build TLS server config")
}

fn server_limits() -> Limits {
    Limits::default()
        .with_enable_connect_protocol(true)
        .with_webtransport(WtInitialSettings {
            initial_max_data: Some(1 << 20),
            initial_max_stream_data_uni: Some(64 * 1024),
            initial_max_stream_data_bidi_local: Some(64 * 1024),
            initial_max_stream_data_bidi_remote: Some(64 * 1024),
            initial_max_streams_uni: Some(10),
            initial_max_streams_bidi: Some(10),
        })
}

/// draft-ietf-webtrans-http2-14: クライアント → サーバー bidi ストリームへの送信をサーバーがエコーし、
/// クライアントで同じデータを受信できることを確認する。
#[tokio::test]
async fn test_wt_bidi_echo() {
    let tls = test_tls();

    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    // サーバータスク
    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");

        let (stream_id, headers) = loop {
            let ev = conn.next_event().await.expect("server next_event");
            if let Event::HeadersReceived {
                stream_id,
                headers,
                end_stream,
                protocol,
            } = ev
            {
                assert!(
                    !end_stream,
                    "WebTransport CONNECT should not set END_STREAM"
                );
                assert_eq!(
                    protocol.as_deref(),
                    Some(b"webtransport" as &[u8]),
                    "protocol should be webtransport"
                );
                break (stream_id, headers);
            }
        };

        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req.accept(WtConfig::default()).await.expect("wt accept");

        // bidi を 1 つ受け取ってエコーする
        let mut bidi = session.accept_bidi().await.expect("bidi");
        let data = bidi.recv().await.expect("recv").expect("data");
        bidi.send(data, true).await.expect("send");

        // セッション終了まで driver を回す
        // (テストでは close を明示的に呼ばず drop で終わる)
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    // クライアント側: 自前で WebTransport を組む
    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");

    // サーバーの SETTINGS を待つ
    loop {
        let ev = client.next_event().await.expect("client next_event");
        if matches!(ev, Event::SettingsReceived { ack: false }) {
            break;
        }
    }

    // Extended CONNECT
    let request = vec![
        HeaderField::from_str(":method", "CONNECT"),
        HeaderField::from_str(":scheme", "https"),
        HeaderField::from_str(":path", "/"),
        HeaderField::from_str(":authority", "localhost"),
        HeaderField::from_str(":protocol", "webtransport"),
    ];
    let connect_stream = client
        .send_request(request, false)
        .await
        .expect("send CONNECT");

    // 200 レスポンス受信
    loop {
        let ev = client.next_event().await.expect("client event");
        if let Event::HeadersReceived { stream_id, .. } = ev {
            assert_eq!(stream_id, connect_stream);
            break;
        }
    }

    // クライアント側 WtSession
    let mut wt_client = WtSession::client(WtConfig::default());
    wt_client.initiate().expect("wt initiate");

    let bidi_id = wt_client.open_bidi_stream().expect("open bidi");
    wt_client
        .send_stream_data(bidi_id, b"ping-pong", false)
        .expect("send stream data");
    while let Some(out) = wt_client.poll_output() {
        client
            .send_data(connect_stream, out, false)
            .await
            .expect("send data");
    }

    // エコー受信
    let mut received = Vec::new();
    while received != b"ping-pong" {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("timeout")
            .expect("client event");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
        {
            if stream_id == connect_stream {
                wt_client.feed(&data).expect("feed");
                wt_client.process().expect("process");
                while let Some(wt_ev) = wt_client.poll_event() {
                    if let WtEvent::StreamData {
                        stream_id: sid,
                        data,
                        ..
                    } = wt_ev
                    {
                        if sid == bidi_id {
                            received.extend_from_slice(&data);
                        }
                    }
                }
            }
        }
    }

    assert_eq!(received, b"ping-pong");
    server_task.abort();
}

/// `WtServerRequest::reject(404)` が動作し、クライアントが 404 を受信することを確認する。
#[tokio::test]
async fn test_wt_reject() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = loop {
            let ev = conn.next_event().await.expect("server event");
            if let Event::HeadersReceived {
                stream_id,
                headers,
                protocol,
                ..
            } = ev
            {
                assert_eq!(protocol.as_deref(), Some(b"webtransport" as &[u8]));
                break (stream_id, headers);
            }
        };
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        req.reject(404).await.expect("reject");
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");

    loop {
        let ev = client.next_event().await.expect("client event");
        if matches!(ev, Event::SettingsReceived { ack: false }) {
            break;
        }
    }

    let request = vec![
        HeaderField::from_str(":method", "CONNECT"),
        HeaderField::from_str(":scheme", "https"),
        HeaderField::from_str(":path", "/"),
        HeaderField::from_str(":authority", "localhost"),
        HeaderField::from_str(":protocol", "webtransport"),
    ];
    let connect_stream = client
        .send_request(request, false)
        .await
        .expect("send CONNECT");

    let mut got_404 = false;
    for _ in 0..10 {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("timeout")
            .expect("client event");
        if let Event::HeadersReceived {
            stream_id, headers, ..
        } = ev
        {
            if stream_id == connect_stream {
                let status = headers
                    .iter()
                    .find(|h| h.name == b":status")
                    .expect("status header")
                    .value
                    .clone();
                assert_eq!(status.as_slice(), b"404");
                got_404 = true;
                break;
            }
        }
    }
    assert!(got_404);
    server_task.await.expect("server join");
}
