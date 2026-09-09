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
        HeaderField::new(":method", "CONNECT").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":protocol", "webtransport").expect("valid header field"),
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
        PrivateKeyDer::try_from(signing_key.serialize_der()).expect("should succeed"),
    )
    .expect("failed to build TLS server config")
}

fn server_limits() -> Limits {
    Limits::builder()
        .enable_connect_protocol(true)
        .wt_enabled(true)
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

/// ローカル開始 bidi の受信上限 (`bidi_local`) を非ゼロ、ピア開始 bidi の
/// 受信上限 (`bidi_remote`) を 0 にした非対称 Limits
///
/// `initial_max_stream_data_bidi_remote = 0` により、ローカル開始 bidi に
/// `bidi_remote` を誤用すると受信ウィンドウが拡張されなくなる。
fn asymmetric_server_limits() -> Limits {
    Limits::builder()
        .enable_connect_protocol(true)
        .wt_enabled(true)
        .webtransport(
            Some(1 << 20),
            Some(64 * 1024),
            Some(64 * 1024),
            Some(10),
            Some(10),
            Some(0),
        )
        .build()
        .expect("非対称 Limits の構築に失敗した")
}

/// draft-ietf-webtrans-http2-15: クライアント → サーバー bidi ストリームへの送信をサーバーがエコーし、
/// クライアントで同じデータを受信できることを確認する。
#[tokio::test]
async fn test_wt_bidi_echo() {
    let tls = test_tls();

    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
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
            .accept(WtConfig::default(), None, None)
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
        HeaderField::new(":method", "CONNECT").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":protocol", "webtransport").expect("valid header field"),
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
    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
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
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
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
        HeaderField::new(":method", "CONNECT").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":protocol", "webtransport").expect("valid header field"),
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
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None, None)
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

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
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
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None, None)
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

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
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
    assert_eq!(received.expect("should succeed"), b"dgram-payload");
    server_task.await.expect("server join");
}

/// close: サーバーが WT_CLOSE_SESSION を送り、クライアントが SessionClosed を受信する
#[tokio::test]
async fn test_wt_close() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("wt accept");
        session.close(99, "shutdown").await.expect("close");
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
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
    let (code, reason) = closed.expect("should succeed");
    assert_eq!(code, 99);
    assert_eq!(reason, "shutdown");
    server_task.await.expect("server join");
}

/// close(): driver が既に落ちている場合にエラーを返す
///
/// クライアントが CONNECT ストリームを END_STREAM で閉じると driver が終了する。
/// その後に close() を呼ぶと `cmd_tx.send()` が失敗し、`Error::ConnectionClosed` を返す。
#[tokio::test]
async fn test_wt_close_errors_when_driver_dead() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("wt accept");
        // クライアントの END_STREAM を driver が処理して終了するのを待つ
        // (同ファイルの他の driver 処理待ちと同じ 2 秒を使う)
        tokio::time::sleep(Duration::from_secs(2)).await;
        let err = session
            .close(0, "done")
            .await
            .expect_err("driver が落ちているので close() は失敗する");
        assert!(
            matches!(err, tokio_http2::Error::ConnectionClosed),
            "close() は ConnectionClosed を返すこと: {err:?}"
        );
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;
    // CONNECT ストリームを END_STREAM で閉じて driver を終了させる
    client
        .send_data(connect_stream, vec![], true)
        .await
        .expect("send END_STREAM");
    server_task.await.expect("server join");
}

/// コマンド処理中の出力フラッシュ失敗が `Error::ConnectionClosed` に丸められず、
/// 呼び出し側へ実際の失敗原因が伝わることを確認する。
///
/// 送信バッファの固定容量 (65535) を超える DATAGRAM を
/// 送信すると、driver の出力フラッシュが sans-io 層のエラー
/// (send buffer full) で失敗する。
#[tokio::test]
async fn test_wt_command_flush_error_not_masked_as_connection_closed() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("wt accept");

        // 送信バッファの固定容量 (65535) を超える DATAGRAM を送ると、出力フラッシュが失敗する
        let err = session
            .send_datagram(vec![0u8; 200_000])
            .await
            .expect_err("出力フラッシュが失敗するので send_datagram はエラーになる");
        assert!(
            matches!(&err, tokio_http2::Error::Protocol(_)),
            "フラッシュ失敗は Error::Protocol として伝わること: {err}"
        );
        assert!(
            format!("{err}").contains("send buffer full"),
            "実際の失敗原因が伝わること: {err}"
        );
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    // WINDOW_UPDATE を送らないままサーバーの処理完了を待つ
    let _connect_stream = perform_connect(&mut client).await;

    server_task.await.expect("server join");
}

/// ローカル開始 bidi ストリームの自動ウィンドウ拡張が
/// `initial_max_stream_data_bidi_local` を基準に動作することを確認する
/// (draft-ietf-webtrans-http2-15 Section 11.2)。
#[tokio::test]
async fn test_wt_local_bidi_window_grows_with_asymmetric_limits() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("アドレスのパースに失敗した"),
        tls,
        asymmetric_server_limits(),
    )
    .await
    .expect("サーバーのバインドに失敗した");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続の受け入れに失敗した");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("WebTransport セッションの受け入れに失敗した");

        // ローカル開始 bidi を開き、クライアントへストリームを知らせる
        let bidi = session
            .open_bidi()
            .await
            .expect("bidi ストリームを開けない");
        bidi.send(b"hello".to_vec(), false)
            .await
            .expect("hello の送信に失敗した");
        // クライアントのデータ受信で maybe_grow_stream_window が動く
        tokio::time::sleep(Duration::from_millis(300)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗した");
    let connect_stream = perform_connect(&mut client).await;

    // サーバーが広告する非対称値を peer_config に反映する
    // (asymmetric_server_limits の bidi_local / bidi_remote と一致させること)
    let peer_config = WtConfig {
        initial_max_stream_data_bidi_local: 64 * 1024,
        initial_max_stream_data_bidi_remote: 0,
        ..WtConfig::default()
    };
    let mut wt_client = WtSession::client(WtConfig::default(), peer_config);
    wt_client.initiate().expect("セッション開始に失敗した");

    // サーバーが開始した bidi ストリーム (クライアント視点ではピア開始) を認識する
    let mut bidi_id = None;
    for _ in 0..50 {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("クライアントイベントの受信がタイムアウトした")
            .expect("クライアントイベントの受信に失敗した");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed に失敗した");
            wt_client.process().expect("process に失敗した");
            while let Some(wt_ev) = wt_client.poll_event() {
                if let WtEvent::StreamOpened { stream_id, .. } = wt_ev {
                    bidi_id = Some(stream_id);
                }
            }
        }
        if bidi_id.is_some() {
            break;
        }
    }
    let bidi_id = bidi_id.expect("サーバー開始 bidi ストリームを認識できなかった");

    let before = wt_client
        .stream(bidi_id)
        .expect("ストリームが存在しない")
        .send_available();
    assert_eq!(before, 64 * 1024, "初期送信ウィンドウは bidi_local のはず");

    // bidi_local (64KiB) の半分を超えるデータを送り、ウィンドウ拡張を誘発する
    wt_client
        .send_stream_data(bidi_id, &vec![0u8; 40 * 1024], false)
        .expect("40KiB の送信に失敗した");
    let out = wt_client.poll_output().expect("出力が無い");
    client
        .send_data(connect_stream, out, false)
        .await
        .expect("データの送信に失敗した");

    // サーバーの WT_MAX_STREAM_DATA を処理して送信ウィンドウが増えることを確認する
    let mut grown = false;
    for _ in 0..50 {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("クライアントイベントの受信がタイムアウトした")
            .expect("クライアントイベントの受信に失敗した");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed に失敗した");
            wt_client.process().expect("process に失敗した");
            while wt_client.poll_event().is_some() {}
            if wt_client
                .stream(bidi_id)
                .expect("ストリームが存在しない")
                .send_available()
                > before
            {
                grown = true;
                break;
            }
        }
    }
    assert!(grown, "サーバー開始 bidi の送信ウィンドウが拡張されるはず");
    server_task.await.expect("サーバータスクの終了に失敗した");
}

/// STOP_SENDING 送信後にピアから在路データが届いてもセッションが継続し、
/// 停止要求後のデータがアプリへ配送されないことを確認する
/// (RFC 9000 Section 3.5 / draft-ietf-webtrans-http2-15 Section 6.3)。
#[tokio::test]
async fn test_wt_stop_sending_inflight_data_does_not_abort_session() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("アドレスのパースに失敗した"),
        tls,
        server_limits(),
    )
    .await
    .expect("サーバーのバインドに失敗した");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続の受け入れに失敗した");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("WebTransport セッションの受け入れに失敗した");

        // ピア開始 bidi ストリームの最初のデータ受信後に STOP_SENDING を送る
        let mut bidi = session
            .accept_bidi()
            .await
            .expect("bidi ストリームの受け入れに失敗した");
        let first = bidi
            .recv()
            .await
            .expect("受信に失敗した")
            .expect("データが無い");
        assert_eq!(first, b"hi");
        bidi.stop_sending(0).await.expect("stop_sending に失敗した");

        // 在路データが処理されるのを待つ
        tokio::time::sleep(Duration::from_millis(300)).await;

        // セッションが継続していること (abort していれば driver が終了してエラーになる)
        session
            .send_datagram(b"alive".to_vec())
            .await
            .expect("セッションは在路データ後も継続するはず");

        // 停止要求後のデータがアプリへ配送されていないこと
        // (チャネルには最初の "hi" 以外は届かない。追加の recv はタイムアウトするか
        // リセットで終了する)
        let extra = tokio::time::timeout(Duration::from_millis(200), bidi.recv()).await;
        assert!(
            !matches!(extra, Ok(Ok(Some(_)))),
            "STOP_SENDING 後のデータは配送されないはず: {extra:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗した");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
    wt_client.initiate().expect("セッション開始に失敗した");
    let bidi_id = wt_client
        .open_bidi_stream()
        .expect("bidi ストリームを開けない");

    // 最初のチャンクを送り、サーバーにストリームを認識させ STOP_SENDING を送らせる
    wt_client
        .send_stream_data(bidi_id, b"hi", false)
        .expect("最初のデータ送信に失敗した");
    let out = wt_client.poll_output().expect("出力が無い");
    client
        .send_data(connect_stream, out, false)
        .await
        .expect("最初のデータの送信に失敗した");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // サーバーの STOP_SENDING を処理する前に在路データを送る
    let inflight = vec![0u8; 40 * 1024];
    wt_client
        .send_stream_data(bidi_id, &inflight, false)
        .expect("在路データの送信に失敗した");
    let out = wt_client.poll_output().expect("出力が無い");
    client
        .send_data(connect_stream, out, false)
        .await
        .expect("在路データの送信に失敗した");

    // サーバーが送る "alive" datagram を受信できること (セッション継続の確認)
    let mut alive = false;
    for _ in 0..50 {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("クライアントイベントの受信がタイムアウトした")
            .expect("クライアントイベントの受信に失敗した");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed に失敗した");
            wt_client.process().expect("process に失敗した");
            while let Some(wt_ev) = wt_client.poll_event() {
                if let WtEvent::DatagramReceived { data } = wt_ev
                    && data == b"alive"
                {
                    alive = true;
                }
            }
        }
        if alive {
            break;
        }
    }
    assert!(
        alive,
        "セッションが継続していれば alive datagram を受信できるはず"
    );
    server_task.await.expect("サーバータスクの終了に失敗した");
}

/// STOP_SENDING 送信後、同じ bidi ストリームを reset した後でも在路データで
/// セッションが abort されないことを確認する (RFC 9000 Section 3.5 は
/// 双方向の終了に RESET_STREAM と STOP_SENDING の併用を想定する)。
#[tokio::test]
async fn test_wt_stop_sending_then_reset_inflight_data_does_not_abort_session() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("アドレスのパースに失敗した"),
        tls,
        server_limits(),
    )
    .await
    .expect("サーバーのバインドに失敗した");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続の受け入れに失敗した");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("WebTransport セッションの受け入れに失敗した");

        let mut bidi = session
            .accept_bidi()
            .await
            .expect("bidi ストリームの受け入れに失敗した");
        let first = bidi
            .recv()
            .await
            .expect("受信に失敗した")
            .expect("データが無い");
        assert_eq!(first, b"hi");
        bidi.stop_sending(0).await.expect("stop_sending に失敗した");
        // stop_sending 後に同じストリームを reset しても在路データの破棄を維持する
        bidi.reset(0).await.expect("reset に失敗した");

        tokio::time::sleep(Duration::from_millis(300)).await;

        session
            .send_datagram(b"alive".to_vec())
            .await
            .expect("セッションは在路データ後も継続するはず");
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗した");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
    wt_client.initiate().expect("セッション開始に失敗した");
    let bidi_id = wt_client
        .open_bidi_stream()
        .expect("bidi ストリームを開けない");

    wt_client
        .send_stream_data(bidi_id, b"hi", false)
        .expect("最初のデータ送信に失敗した");
    let out = wt_client.poll_output().expect("出力が無い");
    client
        .send_data(connect_stream, out, false)
        .await
        .expect("最初のデータの送信に失敗した");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // サーバーの STOP_SENDING / RESET_STREAM を処理する前に在路データを送る
    let inflight = vec![0u8; 40 * 1024];
    wt_client
        .send_stream_data(bidi_id, &inflight, false)
        .expect("在路データの送信に失敗した");
    let out = wt_client.poll_output().expect("出力が無い");
    client
        .send_data(connect_stream, out, false)
        .await
        .expect("在路データの送信に失敗した");

    let mut alive = false;
    for _ in 0..50 {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("クライアントイベントの受信がタイムアウトした")
            .expect("クライアントイベントの受信に失敗した");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed に失敗した");
            wt_client.process().expect("process に失敗した");
            while let Some(wt_ev) = wt_client.poll_event() {
                if let WtEvent::DatagramReceived { data } = wt_ev
                    && data == b"alive"
                {
                    alive = true;
                }
            }
        }
        if alive {
            break;
        }
    }
    assert!(
        alive,
        "セッションが継続していれば alive datagram を受信できるはず"
    );
    server_task.await.expect("サーバータスクの終了に失敗した");
}

/// ピア開始 uni ストリームで STOP_SENDING 後に FIN 付きデータを受信しても
/// アプリへ配送されず、セッションが継続することを確認する。
/// (uni では poll_event が先にストリームを削除するため、driver 側の記録が必要)
#[tokio::test]
async fn test_wt_stop_sending_uni_fin_data_is_discarded() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("アドレスのパースに失敗した"),
        tls,
        server_limits(),
    )
    .await
    .expect("サーバーのバインドに失敗した");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続の受け入れに失敗した");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("WebTransport セッションの受け入れに失敗した");

        let mut uni = session
            .accept_uni()
            .await
            .expect("uni ストリームの受け入れに失敗した");
        let first = uni
            .recv()
            .await
            .expect("受信に失敗した")
            .expect("データが無い");
        assert_eq!(first, b"hi");
        uni.stop_sending(0).await.expect("stop_sending に失敗した");

        tokio::time::sleep(Duration::from_millis(300)).await;

        // FIN 付きデータは配送されず、チャネルが閉じる
        let extra = tokio::time::timeout(Duration::from_millis(200), uni.recv()).await;
        assert!(
            matches!(extra, Ok(Ok(None))),
            "STOP_SENDING 後の FIN 付きデータは配送されずチャネルが閉じるはず: {extra:?}"
        );

        session
            .send_datagram(b"alive".to_vec())
            .await
            .expect("セッションは FIN 付きデータ後も継続するはず");
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗した");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
    wt_client.initiate().expect("セッション開始に失敗した");
    let uni_id = wt_client
        .open_uni_stream()
        .expect("uni ストリームを開けない");

    wt_client
        .send_stream_data(uni_id, b"hi", false)
        .expect("最初のデータ送信に失敗した");
    let out = wt_client.poll_output().expect("出力が無い");
    client
        .send_data(connect_stream, out, false)
        .await
        .expect("最初のデータの送信に失敗した");
    tokio::time::sleep(Duration::from_millis(200)).await;

    wt_client
        .send_stream_data(uni_id, b"fin", true)
        .expect("FIN データの送信に失敗した");
    let out = wt_client.poll_output().expect("出力が無い");
    client
        .send_data(connect_stream, out, false)
        .await
        .expect("FIN データの送信に失敗した");

    let mut alive = false;
    for _ in 0..50 {
        let ev = tokio::time::timeout(Duration::from_secs(5), client.next_event())
            .await
            .expect("クライアントイベントの受信がタイムアウトした")
            .expect("クライアントイベントの受信に失敗した");
        if let Event::DataReceived {
            stream_id, data, ..
        } = ev
            && stream_id == connect_stream
        {
            wt_client.feed(&data).expect("feed に失敗した");
            wt_client.process().expect("process に失敗した");
            while let Some(wt_ev) = wt_client.poll_event() {
                if let WtEvent::DatagramReceived { data } = wt_ev
                    && data == b"alive"
                {
                    alive = true;
                }
            }
        }
        if alive {
            break;
        }
    }
    assert!(
        alive,
        "セッションが継続していれば alive datagram を受信できるはず"
    );
    server_task.await.expect("サーバータスクの終了に失敗した");
}

/// drain: サーバーが WT_DRAIN_SESSION を送り、クライアントが SessionDraining を受信する
#[tokio::test]
async fn test_wt_drain() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let mut session = req
            .accept(WtConfig::default(), None, None)
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

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
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
/// draft-ietf-webtrans-http2-15 Section 6.12 (L1405-L1406) の MUST 要件:
/// WT_CLOSE_SESSION 送信後は END_STREAM で half-close しなければならない。
#[tokio::test]
async fn test_wt_close_sends_end_stream() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("wt accept");
        session.close(0, "done").await.expect("close");
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
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
/// draft-ietf-webtrans-http2-15 Section 6.12 (L1407-L1409):
/// WT_CLOSE_SESSION の受信者は END_STREAM で応答しなければならない (MUST)。
#[tokio::test]
async fn test_wt_close_received_sends_end_stream() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let _session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("wt accept");
        // ドライバーが WT_CLOSE_SESSION を処理し END_STREAM を返信するのを待つ
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
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
/// 先に END_STREAM を返信してから driver が終了すること。
#[tokio::test]
async fn test_wt_close_same_frame_end_stream() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let _session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("wt accept");
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("connect");
    let connect_stream = perform_connect(&mut client).await;

    let mut wt_client = WtSession::client(WtConfig::default(), WtConfig::default());
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
        HeaderField::new(":method", "CONNECT").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":protocol", "webtransport").expect("valid header field"),
        HeaderField::new("webtransport-init", init).expect("valid header field"),
    ]
}

/// Origin ヘッダーつきの CONNECT 要求ヘッダー
fn connect_request_with_origin(origin: &str) -> Vec<HeaderField> {
    vec![
        HeaderField::new(":method", "CONNECT").expect("valid header field"),
        HeaderField::new(":scheme", "https").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
        HeaderField::new(":authority", "localhost").expect("valid header field"),
        HeaderField::new(":protocol", "webtransport").expect("valid header field"),
        HeaderField::new("origin", origin).expect("valid header field"),
    ]
}

/// 許可された Origin と一致する場合は 200 で受理されることを確認する。
#[tokio::test]
async fn test_wt_origin_allowed() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let _session = req
            .accept(WtConfig::default(), Some(b"https://example.com"), None)
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
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        match req
            .accept(WtConfig::default(), Some(b"https://example.com"), None)
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

/// draft-ietf-webtrans-http2-15 Section 7 (L1483-L1487):
/// TLS 1.3 で WebTransport セッションを要求した場合は `accept()` が成功する。
/// (既存テストでも TLS 1.3 経路は通っているが、リグレッション防止のため明示テストを置く)
#[tokio::test]
async fn test_wt_tls13_accept() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        // TLS 1.3 がネゴシエートされているため accept() は成功する想定
        let _session = req
            .accept(WtConfig::default(), None, None)
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

/// draft-ietf-webtrans-http2-15 Section 7 (L1483-L1487) + RFC 9113 Section 8.1.1 (L2463-L2465):
/// TLS 1.2 で WebTransport セッションを要求した場合は malformed として扱い、
/// CONNECT ストリームに `RST_STREAM(PROTOCOL_ERROR)` を送出して拒否する。
#[tokio::test]
async fn test_wt_tls12_rejected() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
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
        match req.accept(WtConfig::default(), None, None).await {
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
                ..
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

/// Origin ヘッダーが存在しない場合、allowed_origin=Some でも accept が成功することを確認する。
/// (draft-ietf-webtrans-http2-15 Section 3.2: Origin 検証はヘッダーがある場合のみ)
#[tokio::test]
async fn test_wt_origin_missing_accepted() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("bind");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("accept");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        // Origin 欠落時は検証スキップ → accept 成功
        req.accept(WtConfig::default(), Some(b"https://example.com"), None)
            .await
            .expect("missing origin should be accepted (verification skipped)");
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

    // サーバー側の成功チェックのみで十分
    server_task.await.expect("server join");
}

/// draft-ietf-webtrans-http2-15 Section 4.3 (L524-L528):
/// WebTransport-Init で SETTINGS より大きい値を送ると `accept()` が成功し、
/// セッションが確立できる (パースが成功する経路の確認)。
#[tokio::test]
async fn test_wt_init_accept_with_large_value() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
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
            .accept(WtConfig::default(), None, None)
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
/// (apply_init_as_peer の max マージで実値は SETTINGS 由来のまま維持される)。
#[tokio::test]
async fn test_wt_init_accept_with_small_value() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let _session = req
            .accept(WtConfig::default(), None, None)
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

/// draft-ietf-webtrans-http2-15 Section 4.3.2 (L583-L590):
/// WebTransport-Init のパース失敗 (負値) で `:status=400` レスポンスが返り、
/// CONNECT ストリームが END_STREAM で閉じられる。
#[tokio::test]
async fn test_wt_init_rejected_on_invalid_value() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        match req.accept(WtConfig::default(), None, None).await {
            Ok(_) => panic!("負値の WebTransport-Init で accept() が成功してしまった"),
            Err(err) => {
                assert!(
                    matches!(err, tokio_http2::Error::WebTransport(_)),
                    "WebTransport-Init パース失敗は Error::WebTransport が期待だが、実際は {err}"
                );
            }
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

/// `wt-available-protocols` つきの CONNECT 要求ヘッダー
fn connect_request_with_protocols(protocols: &str) -> Vec<HeaderField> {
    let mut h = connect_request();
    h.push(HeaderField::new("wt-available-protocols", protocols).expect("valid"));
    h
}

/// `:scheme` を差し替えた CONNECT 要求ヘッダー
fn connect_request_with_scheme(scheme: &str) -> Vec<HeaderField> {
    let mut h = connect_request();
    for field in &mut h {
        if field.name() == b":scheme" {
            *field = HeaderField::new(":scheme", scheme).expect("valid scheme");
            return h;
        }
    }
    panic!(":scheme ヘッダーが見つからない");
}

/// draft-ietf-webtrans-http2-15 Section 3.3:
/// `selected_protocol = Some(b"echo")` かつ `wt-available-protocols: "echo"` なら
/// 受理され、レスポンスに `wt-protocol` (sf-string) が付く。
#[tokio::test]
async fn test_wt_selected_protocol_accepted() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        assert_eq!(
            req.wt_available_protocols(),
            Some(br#""echo""# as &[u8]),
            "wt-available-protocols が取得できること"
        );
        let session = req
            .accept(WtConfig::default(), None, Some(b"echo"))
            .await
            .expect("リスト内の selected_protocol なら accept は成功すべき");
        assert_eq!(
            session.selected_protocol(),
            Some(b"echo" as &[u8]),
            "selected_protocol が保持されること"
        );
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
    let connect_stream = client
        .send_request(connect_request_with_protocols(r#""echo""#), false)
        .await
        .expect("CONNECT 送信に失敗");

    let mut got_wt_protocol = false;
    let recv = async {
        loop {
            let ev = client
                .next_event()
                .await
                .expect("クライアントイベント取得に失敗");
            if let Event::HeadersReceived {
                stream_id, headers, ..
            } = ev
                && stream_id == connect_stream
            {
                let status = headers
                    .iter()
                    .find(|h| h.name() == b":status")
                    .expect("status ヘッダーが必要")
                    .value();
                assert_eq!(status, b"200", "期待: 200");
                let wt_protocol = headers
                    .iter()
                    .find(|h| h.name() == b"wt-protocol")
                    .expect("wt-protocol ヘッダーが必要")
                    .value();
                assert_eq!(
                    wt_protocol, br#""echo""#,
                    "wt-protocol は sf-string \"echo\" であること"
                );
                got_wt_protocol = true;
                break;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), recv)
        .await
        .expect("タイムアウト");

    server_task.await.expect("サーバータスクの join に失敗");
    assert!(got_wt_protocol, "wt-protocol を受信しなかった");
}

/// draft-ietf-webtrans-http2-15 Section 3.3:
/// `selected_protocol` が `WT-Available-Protocols` に含まれない場合は
/// `Error::InvalidArgument` を返し、レスポンスは送らない。
#[tokio::test]
async fn test_wt_selected_protocol_not_listed() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        match req.accept(WtConfig::default(), None, Some(b"raw")).await {
            Ok(_) => panic!("リスト外の selected_protocol で accept が成功してしまった"),
            Err(err) => {
                assert!(
                    matches!(err, tokio_http2::Error::InvalidArgument(_)),
                    "リスト外は InvalidArgument が期待だが、実際は {err}"
                );
            }
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
    let _connect_stream = client
        .send_request(connect_request_with_protocols(r#""echo""#), false)
        .await
        .expect("CONNECT 送信に失敗");

    server_task.await.expect("サーバータスクの join に失敗");
}

/// draft-ietf-webtrans-http2-15 Section 5.3:
/// `export_keying_material` が指定長の鍵素材を返し、冪等であり、label 変更で異なる。
#[tokio::test]
async fn test_wt_export_keying_material() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("バインドに失敗");
    let addr = server.local_addr();

    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        let (stream_id, headers) = await_connect_headers(&mut conn).await;
        let req = WtServerRequest::from_connection(conn, stream_id, headers);
        let session = req
            .accept(WtConfig::default(), None, None)
            .await
            .expect("accept は成功すべき");

        let a = session
            .export_keying_material(b"label", b"ctx", 32)
            .await
            .expect("export は成功すべき");
        assert_eq!(a.len(), 32, "length=32 なら 32 バイト返すこと");

        let b = session
            .export_keying_material(b"label", b"ctx", 32)
            .await
            .expect("2 回目の export も成功すべき");
        assert_eq!(a, b, "同一引数なら冪等であること");

        let c = session
            .export_keying_material(b"other", b"ctx", 32)
            .await
            .expect("label 変更後の export も成功すべき");
        assert_ne!(a, c, "label が異なれば鍵素材も異なること");

        match session.export_keying_material(b"label", b"ctx", 0).await {
            Ok(_) => panic!("length=0 で export が成功してしまった"),
            Err(err) => {
                assert!(
                    matches!(err, tokio_http2::Error::InvalidArgument(_)),
                    "length=0 は InvalidArgument が期待だが、実際は {err}"
                );
            }
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    let mut client = Client::connect_insecure(addr, "localhost", Limits::default())
        .await
        .expect("接続に失敗");
    let _connect_stream = perform_connect(&mut client).await;
    server_task.await.expect("サーバータスクの join に失敗");
}

/// draft-ietf-webtrans-http2-15 Section 3.2:
/// `:scheme` が `http` の `:protocol=webtransport` CONNECT は Sans I/O 層で送信時に拒否される。
#[tokio::test]
async fn test_wt_scheme_http_rejected() {
    let tls = test_tls();
    let server = Server::bind(
        "127.0.0.1:0".parse().expect("parse should succeed"),
        tls,
        server_limits(),
    )
    .await
    .expect("バインドに失敗");
    let addr = server.local_addr();

    // サーバーは接続を維持するだけ (クライアント側で送信が拒否されるため)
    let server_task = tokio::spawn(async move {
        let mut conn = server.accept().await.expect("接続受け入れに失敗");
        // クライアントの SETTINGS 交換を完了させる
        while conn.next_event().await.is_ok() {}
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
    // Sans I/O 層の検証で送信時に拒否される
    let result = client
        .send_request(connect_request_with_scheme("http"), false)
        .await;
    assert!(
        result.is_err(),
        ":protocol=webtransport + :scheme=http は送信時に拒否されるべき"
    );

    server_task.abort();
}
