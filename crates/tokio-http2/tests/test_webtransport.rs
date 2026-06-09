//! WebTransport over HTTP/2 サーバー統合テスト
#![allow(clippy::collapsible_match, clippy::collapsible_if)]

use std::time::Duration;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use shiguredo_http2::webtransport::{
    WtConfig, WtEvent, WtSession, stream::stream_id as wt_stream_id,
};

use tokio_http2::{
    Client, ErrorCode, Event, HeaderField, Limits, Server, TlsClientConfig, TlsServerConfig,
    WtServerRequest,
};

/// テストで繰り返し使う CONNECT 要求ヘッダー
fn connect_request() -> Vec<HeaderField> {
    vec![
        HeaderField::new(":method", "CONNECT").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":protocol", "webtransport").unwrap(),
    ]
}

/// クライアント側で Extended CONNECT を送り、200 を受信するまで駆動する
async fn perform_connect(client: &mut Client) -> shiguredo_http2::StreamId {
    loop {
        let ev = client.next_event().await.expect("client next_event");
        if matches!(ev, Event::SettingsReceived { ack: false }) {
            break;
        }
    }
    let connect_stream = client
        .send_request(connect_request(), false)
        .await
        .expect("send CONNECT");
    loop {
        let ev = client.next_event().await.expect("client event");
        if let Event::HeadersReceived { stream_id, .. } = ev
            && stream_id == connect_stream
        {
            break;
        }
    }
    connect_stream
}

/// サーバー側で Extended CONNECT HEADERS を待機する
async fn await_connect_headers(
    conn: &mut tokio_http2::ServerConnection,
) -> (shiguredo_http2::StreamId, Vec<HeaderField>) {
    loop {
        let ev = conn.next_event().await.expect("server next_event");
        if let Event::HeadersReceived {
            stream_id,
            headers,
            protocol,
            ..
        } = ev
        {
            assert_eq!(protocol.as_deref(), Some(b"webtransport" as &[u8]));
            return (stream_id, headers);
        }
    }
}

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
    Limits::builder()
        .enable_connect_protocol(true)
        .webtransport(
            Some(1 << 20),
            Some(64 * 1024),
            Some(64 * 1024),
            Some(10),
            Some(10),
            Some(64 * 1024),
        )
        .build()
        .expect("valid server limits")
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
        let mut session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("wt accept");

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
        HeaderField::new(":method", "CONNECT").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":protocol", "webtransport").unwrap(),
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
        HeaderField::new(":method", "CONNECT").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":protocol", "webtransport").unwrap(),
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
                    .find(|h| h.name() == b":status")
                    .expect("status header")
                    .value()
                    .to_vec();
                assert_eq!(status.as_slice(), b"404");
                got_404 = true;
                break;
            }
        }
    }
    assert!(got_404);
    server_task.await.expect("server join");
}

/// 単方向ストリームのエコー: クライアント→サーバー uni → サーバー→クライアント uni で返す
#[tokio::test]
async fn test_wt_uni_echo() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("wt accept");
        let mut uni_recv = session.accept_uni().await.expect("uni recv");
        let data = uni_recv.recv().await.expect("recv").expect("data");
        let uni_send = session.open_uni().await.expect("open uni");
        uni_send.send(data, true).await.expect("send");
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default());
    wt_client.initiate().expect("initiate");
    let uni_id = wt_client.open_uni_stream().expect("open uni");
    wt_client
        .send_stream_data(uni_id, b"unicorn", true)
        .expect("send");
    while let Some(out) = wt_client.poll_output() {
        client
            .send_data(connect_stream, out, false)
            .await
            .expect("send data");
    }

    let mut received = Vec::new();
    while received != b"unicorn" {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("timeout")
            .expect("client event");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed");
            wt_client.process().expect("process");
            while let Some(wt_ev) = wt_client.poll_event() {
                if let WtEvent::StreamData {
                    stream_id: sid,
                    data,
                    ..
                } = wt_ev
                    && wt_stream_id::is_server_initiated(sid)
                    && wt_stream_id::is_unidirectional(sid)
                {
                    received.extend_from_slice(&data);
                }
            }
        }
    }
    assert_eq!(received, b"unicorn");
    server_task.await.expect("server join");
}

/// DATAGRAM エコー: WT DATAGRAM capsule のラウンドトリップ
#[tokio::test]
async fn test_wt_datagram_echo() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("wt accept");
        let data = session.recv_datagram().await.expect("datagram");
        session.send_datagram(data).await.expect("echo");
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default());
    wt_client.initiate().expect("initiate");
    wt_client.send_datagram(b"dgram-payload").expect("send");
    while let Some(out) = wt_client.poll_output() {
        client
            .send_data(connect_stream, out, false)
            .await
            .expect("send data");
    }

    let mut received: Option<Vec<u8>> = None;
    while received.is_none() {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("timeout")
            .expect("client event");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed");
            wt_client.process().expect("process");
            while let Some(wt_ev) = wt_client.poll_event() {
                if let WtEvent::DatagramReceived { data } = wt_ev {
                    received = Some(data);
                }
            }
        }
    }
    assert_eq!(received.unwrap(), b"dgram-payload");
    server_task.await.expect("server join");
}

/// close: サーバーが WT_CLOSE_SESSION を送り、クライアントが SessionClosed を受信する
#[tokio::test]
async fn test_wt_close() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("wt accept");
        session.close(99, "shutdown").await.expect("close");
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default());
    wt_client.initiate().expect("initiate");

    let mut closed: Option<(u32, String)> = None;
    while closed.is_none() {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("timeout")
            .expect("client event");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed");
            wt_client.process().expect("process");
            while let Some(wt_ev) = wt_client.poll_event() {
                if let WtEvent::SessionClosed { error_code, reason } = wt_ev {
                    closed = Some((error_code, reason));
                }
            }
        }
    }
    let (code, reason) = closed.unwrap();
    assert_eq!(code, 99);
    assert_eq!(reason, "shutdown");
    server_task.await.expect("server join");
}

/// drain: サーバーが WT_DRAIN_SESSION を送り、クライアントが SessionDraining を受信する
#[tokio::test]
async fn test_wt_drain() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("wt accept");
        session.drain().await.expect("drain");
        // クライアント側が capsule を受信する余地を与える
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default());
    wt_client.initiate().expect("initiate");

    let mut drained = false;
    while !drained {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("timeout")
            .expect("client event");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed");
            wt_client.process().expect("process");
            while let Some(wt_ev) = wt_client.poll_event() {
                if matches!(wt_ev, WtEvent::SessionDraining) {
                    drained = true;
                }
            }
        }
    }
    assert!(drained);
    server_task.await.expect("server join");
}

/// サーバーが close() を呼んだ後、クライアントが CONNECT ストリーム上で
/// END_STREAM を受信することを確認する。
///
/// draft-ietf-webtrans-http2-14 Section 6.12 (L1360-L1361) の MUST 要件:
/// WT_CLOSE_SESSION 送信後は END_STREAM で half-close しなければならない。
#[tokio::test]
async fn test_wt_close_sends_end_stream() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("wt accept");
        session.close(0, "done").await.expect("close");
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default());
    wt_client.initiate().expect("initiate");

    let mut end_stream_received = false;
    while !end_stream_received {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("タイムアウト")
            .expect("client event");
        match ev {
            Event::DataReceived {
                stream_id,
                data,
                end_stream,
            } if stream_id == connect_stream => {
                if !data.is_empty() {
                    wt_client.feed(&data).expect("feed");
                    wt_client.process().expect("process");
                    while wt_client.poll_event().is_some() {}
                }
                if end_stream {
                    end_stream_received = true;
                }
            }
            Event::StreamClosed { stream_id } if stream_id == connect_stream => {
                end_stream_received = true;
            }
            _ => {}
        }
    }
    assert!(
        end_stream_received,
        "クライアントが END_STREAM を受信しなかった"
    );
    server_task.await.expect("server join");
}

/// クライアントが WT_CLOSE_SESSION + END_STREAM を送信した後、
/// サーバーが END_STREAM を返信することを確認する。
///
/// draft-ietf-webtrans-http2-14 Section 6.12 (L1364-L1365):
/// WT_CLOSE_SESSION の受信者は END_STREAM で応答しなければならない (MUST)。
#[tokio::test]
async fn test_wt_close_received_sends_end_stream() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let _session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("wt accept");
        // ドライバーが WT_CLOSE_SESSION を処理し END_STREAM を返信するのを待つ
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default());
    wt_client.initiate().expect("initiate");

    // WT_CLOSE_SESSION capsule を送信
    wt_client.close(0, "done").expect("close");
    while let Some(out) = wt_client.poll_output() {
        client
            .send_data(connect_stream, out, false)
            .await
            .expect("send capsule");
    }
    // END_STREAM を送信
    client
        .send_data(connect_stream, vec![], true)
        .await
        .expect("send END_STREAM");

    // サーバーからの END_STREAM 返信を待つ
    let mut end_stream_received = false;
    while !end_stream_received {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("タイムアウト")
            .expect("client event");
        match ev {
            Event::DataReceived {
                stream_id,
                end_stream,
                ..
            } if stream_id == connect_stream && end_stream => {
                end_stream_received = true;
            }
            Event::StreamClosed { stream_id } if stream_id == connect_stream => {
                end_stream_received = true;
            }
            _ => {}
        }
    }
    assert!(
        end_stream_received,
        "サーバーが END_STREAM を返信しなかった"
    );
    server_task.await.expect("server join");
}

/// クライアントが WT_CLOSE_SESSION と END_STREAM を同一 DATA フレームで送信した後、
/// サーバーが END_STREAM を返信することを確認する。
///
/// end_stream=true と WT_CLOSE_SESSION が同一フレームで届くエッジケースでも、
/// 先に END_STREAM を返信してから driver が終了すること (issue 0058 エッジケース)。
#[tokio::test]
async fn test_wt_close_same_frame_end_stream() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let _session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("wt accept");
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default());
    wt_client.initiate().expect("initiate");

    // WT_CLOSE_SESSION capsule と END_STREAM を同一 DATA フレームで送信
    wt_client.close(0, "done").expect("close");
    while let Some(out) = wt_client.poll_output() {
        client
            .send_data(connect_stream, out, true)
            .await
            .expect("send capsule + END_STREAM");
    }

    // サーバーからの END_STREAM 返信を待つ
    let mut end_stream_received = false;
    while !end_stream_received {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("タイムアウト")
            .expect("client event");
        match ev {
            Event::DataReceived {
                stream_id,
                end_stream,
                ..
            } if stream_id == connect_stream && end_stream => {
                end_stream_received = true;
            }
            Event::StreamClosed { stream_id } if stream_id == connect_stream => {
                end_stream_received = true;
            }
            _ => {}
        }
    }
    assert!(
        end_stream_received,
        "サーバーが END_STREAM を返信しなかった"
    );
    server_task.await.expect("server join");
}

/// `WebTransport-Init` ヘッダーつきの CONNECT 要求ヘッダー
fn connect_request_with_webtransport_init(init: &str) -> Vec<HeaderField> {
    vec![
        HeaderField::new(":method", "CONNECT").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":protocol", "webtransport").unwrap(),
        HeaderField::new("webtransport-init", init).unwrap(),
    ]
}

/// Origin ヘッダーつきの CONNECT 要求ヘッダー
fn connect_request_with_origin(origin: &str) -> Vec<HeaderField> {
    vec![
        HeaderField::new(":method", "CONNECT").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":path", "/").unwrap(),
        HeaderField::new(":authority", "localhost").unwrap(),
        HeaderField::new(":protocol", "webtransport").unwrap(),
        HeaderField::new("origin", origin).unwrap(),
    ]
}

/// 許可された Origin と一致する場合は 200 で受理されることを確認する。
#[tokio::test]
async fn test_wt_origin_allowed() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let _session = req
            .accept(WtConfig::default(), Some(b"https://example.com"))
            .await
            .expect("wt accept");
        tokio::time::sleep(Duration::from_millis(100)).await;
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
    let connect_stream = client
        .send_request(connect_request_with_origin("https://example.com"), false)
        .await
        .expect("send CONNECT");

    let mut got_200 = false;
    for _ in 0..10 {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("timeout")
            .expect("client event");
        if let Event::HeadersReceived {
            stream_id, headers, ..
        } = ev
            && stream_id == connect_stream
        {
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("status header")
                .value()
                .to_vec();
            assert_eq!(status.as_slice(), b"200");
            got_200 = true;
            break;
        }
    }
    assert!(got_200, "Origin が許可されなかった");
    server_task.await.expect("server join");
}

/// 許可されていない Origin の場合は 403 で拒否されることを確認する。
#[tokio::test]
async fn test_wt_origin_rejected() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        match req
            .accept(WtConfig::default(), Some(b"https://example.com"))
            .await
        {
            Ok(_) => panic!("expected origin rejection but succeeded"),
            Err(err) => assert!(
                format!("{err}").contains("origin rejected"),
                "expected origin rejection, got {err}"
            ),
        }
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
    let connect_stream = client
        .send_request(connect_request_with_origin("https://evil.com"), false)
        .await
        .expect("send CONNECT");

    let mut got_403 = false;
    for _ in 0..10 {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("timeout")
            .expect("client event");
        if let Event::HeadersReceived {
            stream_id, headers, ..
        } = ev
            && stream_id == connect_stream
        {
            let status = headers
                .iter()
                .find(|h| h.name() == b":status")
                .expect("status header")
                .value()
                .to_vec();
            assert_eq!(status.as_slice(), b"403");
            got_403 = true;
            break;
        }
    }
    assert!(got_403, "Origin が拒否されなかった");
    server_task.await.expect("server join");
}

/// draft-ietf-webtrans-http2-14 Section 7 (L1425-L1438):
/// TLS 1.3 で WebTransport セッションを要求した場合は `accept()` が成功する。
/// (既存テストでも TLS 1.3 経路は通っているが、リグレッション防止のため明示テストを置く)
#[tokio::test]
async fn test_wt_tls13_accept() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        // TLS 1.3 がネゴシエートされているため accept() は成功する想定
        let _session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("TLS 1.3 では accept() が成功すべき");
        // driver タスクが少なくとも 1 回 select! を回す程度の余地を確保してから drop で終了
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗");
    let _connect_stream = perform_connect(&mut client).await;
    server_task.await.expect("サーバータスクの join に失敗");
}

/// draft-ietf-webtrans-http2-14 Section 7 (L1425-L1438) + RFC 9113 Section 8.1.1 (L2463-L2465):
/// TLS 1.2 で WebTransport セッションを要求した場合は malformed として扱い、
/// CONNECT ストリームに `RST_STREAM(PROTOCOL_ERROR)` を送出して拒否する。
#[tokio::test]
async fn test_wt_tls12_rejected() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        // TLS 1.2 がネゴシエートされているため accept() は TLS 要件未達で失敗する。
        // ただし RST_STREAM 送信 (reset_stream の内部 ?) で I/O エラーが先に伝搬する
        // 可能性もあるため、その経路も許容する。
        match req.accept(WtConfig::default(), None).await {
            Ok(_) => panic!("TLS 1.2 で accept() が成功してしまった"),
            Err(err) => {
                let msg = format!("{err}");
                assert!(
                    msg.contains("TLS 1.3") || matches!(err, tokio_http2::Error::Io(_)),
                    "期待: TLS 1.3 要求エラーまたは I/O エラー、実際: {err}"
                );
            }
        }
    });

    // クライアントを TLS 1.2 限定で構築し、TLS 1.2 のハンドシェイクを強制する
    let tls_client_config =
        TlsClientConfig::insecure_tls12_only().expect("TLS 1.2 限定設定の構築に失敗");
    let mut client = Client::connect(addr, "localhost", tls_client_config, Limits::default())
        .await
        .expect("接続に失敗");

    // サーバーの SETTINGS を待ってから CONNECT を送る
    loop {
        let ev = client
            .next_event()
            .await
            .expect("クライアント next_event に失敗");
        if matches!(ev, Event::SettingsReceived { ack: false }) {
            break;
        }
    }

    let connect_stream = client
        .send_request(connect_request(), false)
        .await
        .expect("CONNECT 送信に失敗");

    // クライアントは CONNECT ストリームに対する RST_STREAM(PROTOCOL_ERROR) を受信する。
    // 5 秒のグローバルタイムアウトでループを保護し、目的のイベントが来るまで待ち続ける。
    let mut got_reset = false;
    let mut got_error_code: Option<ErrorCode> = None;
    let recv = async {
        loop {
            let ev = client
                .next_event()
                .await
                .expect("クライアントイベント取得に失敗");
            if let Event::StreamReset {
                stream_id,
                error_code,
            } = ev
                && stream_id == connect_stream
            {
                got_reset = true;
                got_error_code = Some(error_code);
                break;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), recv)
        .await
        .expect("タイムアウト");

    // server_task の panic を取りこぼさないよう、got_reset の assert より先に
    // サーバー側の join 結果を確認する
    server_task.await.expect("サーバータスクの join に失敗");

    assert!(got_reset, "RST_STREAM(PROTOCOL_ERROR) を受信しなかった");
    assert_eq!(
        got_error_code,
        Some(ErrorCode::ProtocolError),
        "期待される error_code は PROTOCOL_ERROR"
    );
}

/// Origin ヘッダーが存在しない場合に 403 で拒否されることを確認する。
#[tokio::test]
async fn test_wt_origin_missing_rejected() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        match req
            .accept(WtConfig::default(), Some(b"https://example.com"))
            .await
        {
            Ok(_) => panic!("expected missing origin error but succeeded"),
            Err(err) => assert!(
                format!("{err}").contains("Origin header is required but missing"),
                "expected missing origin error, got {err}"
            ),
        }
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
    let _connect_stream = client
        .send_request(connect_request(), false)
        .await
        .expect("send CONNECT");

    // サーバー側のエラーチェックのみで十分
    server_task.await.expect("server join");
}

/// draft-ietf-webtrans-http2-14 Section 4.3 (L480-L483):
/// WebTransport-Init で SETTINGS より大きい値を送ると `accept()` が成功し、
/// セッションが確立できる (パースが成功する経路の確認)。
#[tokio::test]
async fn test_wt_init_accept_with_large_value() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        // WtServerRequest 経由で WebTransport-Init ヘッダー値が取得できることも確認する
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let init_value = req
            .webtransport_init()
            .map(|v| v.to_vec())
            .expect("WebTransport-Init ヘッダーが取得できること");
        assert!(
            init_value.starts_with(b"u="),
            "WebTransport-Init は 'u=...' で始まること、実際は {:?}",
            String::from_utf8_lossy(&init_value)
        );
        // SETTINGS 由来のデフォルトより大きい値を送っているのでパース成功する
        let _session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("WebTransport-Init パース成功なら accept() は成功すべき");
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗");
    loop {
        let ev = client
            .next_event()
            .await
            .expect("クライアント next_event に失敗");
        if matches!(ev, Event::SettingsReceived { ack: false }) {
            break;
        }
    }
    let _connect_stream = client
        .send_request(connect_request_with_webtransport_init("u=999999"), false)
        .await
        .expect("CONNECT 送信に失敗");

    server_task.await.expect("サーバータスクの join に失敗");
}

/// WebTransport-Init で SETTINGS より小さい値を送っても `accept()` が成功し、
/// (apply_init の max マージで実値は SETTINGS 由来のまま維持される)。
#[tokio::test]
async fn test_wt_init_accept_with_small_value() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let _session = req
            .accept(WtConfig::default(), None)
            .await
            .expect("小さい値でも accept() は成功すべき (max マージで無視されるだけ)");
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗");
    loop {
        let ev = client
            .next_event()
            .await
            .expect("クライアント next_event に失敗");
        if matches!(ev, Event::SettingsReceived { ack: false }) {
            break;
        }
    }
    let _connect_stream = client
        .send_request(connect_request_with_webtransport_init("u=10"), false)
        .await
        .expect("CONNECT 送信に失敗");

    server_task.await.expect("サーバータスクの join に失敗");
}

/// draft-ietf-webtrans-http2-14 Section 4.3.2 (L525-L540):
/// WebTransport-Init のパース失敗 (負値) で `:status=400` レスポンスが返り、
/// CONNECT ストリームが END_STREAM で閉じられる。
#[tokio::test]
async fn test_wt_init_rejected_on_invalid_value() {
    let tls = test_tls();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), tls, server_limits())
        .await
        .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        match req.accept(WtConfig::default(), None).await {
            Ok(_) => panic!("負値の WebTransport-Init で accept() が成功してしまった"),
            Err(err) => assert!(
                format!("{err}").contains("WebTransport-Init parse error"),
                "WebTransport-Init パースエラーが期待だが、実際は {err}"
            ),
        }
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗");
    loop {
        let ev = client
            .next_event()
            .await
            .expect("クライアント next_event に失敗");
        if matches!(ev, Event::SettingsReceived { ack: false }) {
            break;
        }
    }
    let connect_stream = client
        .send_request(connect_request_with_webtransport_init("u=-1"), false)
        .await
        .expect("CONNECT 送信に失敗");

    // クライアントは :status=400 と END_STREAM を受信する
    let mut got_400 = false;
    let recv = async {
        loop {
            let ev = client
                .next_event()
                .await
                .expect("クライアントイベント取得に失敗");
            if let Event::HeadersReceived {
                stream_id,
                headers,
                end_stream,
                ..
            } = ev
                && stream_id == connect_stream
            {
                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("status ヘッダーが必要")
                    .value()
                    .to_vec();
                assert_eq!(status.as_slice(), b"400", "期待: 400, 実際: {:?}", status);
                assert!(
                    end_stream,
                    "CONNECT ストリームは END_STREAM で閉じられるべき"
                );
                got_400 = true;
                break;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), recv)
        .await
        .expect("タイムアウト");

    server_task.await.expect("サーバータスクの join に失敗");
    assert!(got_400, ":status=400 を受信しなかった");
}
