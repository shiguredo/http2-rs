//! tokio-http2 と tokio-nghttp2 の疎通確認用ヘルパー
//!
//! 実装の組み合わせごとの疎通確認テストで共有する、証明書生成・サーバー起動・
//! クライアント接続・1 往復のヘルパーを提供する。網羅的な回帰ではなく、
//! 最小構成で両実装が疎通することだけを確認する。

use std::net::SocketAddr;
use std::time::Duration;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::task::JoinHandle;
use tokio_http2::{
    Client as Http2Client, Event as Http2Event, HeaderField, Limits, Server as Http2Server,
    ServerConnection as Http2ServerConnection, TlsServerConfig as Http2TlsServerConfig,
};
use tokio_nghttp2::{
    Client as NgClient, Header as NgHeader, Http2Event as NgHttp2Event, Server as NgServer,
    ServerConnection as NgServerConnection, TlsServerConfig as NgTlsServerConfig,
};

/// 疎通確認で返すレスポンスボディ
pub const RESPONSE_BODY: &[u8] = b"interop smoke test body";

/// 待ち受けアドレス
///
/// ポート 0 を指定して OS に空きポートを選ばせる。
const LISTEN_ADDR: &str = "127.0.0.1:0";

/// サーバー名
///
/// SNI と `:authority` に使い、生成する自己署名証明書の SAN と一致させる。
const SERVER_NAME: &str = "localhost";

/// 接続確立と 1 イベントの受信に適用する待機の上限
///
/// 疎通確認が停止したままテストが終わらないことを防ぐ。
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// 応答送信後にピアのクローズを待つ上限
///
/// 未読データを残したまま接続を閉じると OS が RST を送り、送信済みの応答が
/// ピアに届かないことがあるため、ピアのクローズまで読み続ける。その待ちの上限である。
const LINGER_TIMEOUT: Duration = Duration::from_secs(2);

/// 1 往復で受け取ったレスポンス
#[derive(Debug)]
pub struct Response {
    /// `:status` の値
    pub status: Vec<u8>,
    /// レスポンスボディ
    pub body: Vec<u8>,
}

/// tokio-http2 サーバー用の TLS 設定を生成する
///
/// localhost 用の自己署名証明書を生成して渡す。
fn http2_server_tls() -> Http2TlsServerConfig {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec![SERVER_NAME.to_string()])
            .expect("failed to generate a self-signed certificate");

    Http2TlsServerConfig::from_der(
        vec![CertificateDer::from(cert.der().to_vec())],
        PrivateKeyDer::try_from(signing_key.serialize_der())
            .expect("failed to convert the private key"),
    )
    .expect("failed to build the tokio-http2 TLS server config")
}

/// tokio-nghttp2 サーバー用の TLS 設定を生成する
///
/// localhost 用の自己署名証明書を生成して渡す。
fn nghttp2_server_tls() -> NgTlsServerConfig {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec![SERVER_NAME.to_string()])
            .expect("failed to generate a self-signed certificate");

    NgTlsServerConfig::from_der(
        vec![CertificateDer::from(cert.der().to_vec())],
        PrivateKeyDer::try_from(signing_key.serialize_der())
            .expect("failed to convert the private key"),
    )
    .expect("failed to build the tokio-nghttp2 TLS server config")
}

/// ピアが接続を閉じるか `LINGER_TIMEOUT` が経過するまでイベントを読み続ける
///
/// 応答を送信した側が未読データを残したまま接続を閉じると、OS が RST を送って
/// 送信済みの応答がピアに届かないことがある。ピアのクローズまで読み続けて防ぐ。
async fn drain_http2_connection(conn: &mut Http2ServerConnection) {
    let deadline = tokio::time::Instant::now() + LINGER_TIMEOUT;
    // イベントを受信できている間は読み続ける。ピアのクローズ (Err) と
    // LINGER_TIMEOUT の経過 (timeout) でループを抜ける
    while let Ok(Ok(_)) = tokio::time::timeout_at(deadline, conn.next_event()).await {}
}

/// [`drain_http2_connection`] の tokio-nghttp2 版
async fn drain_nghttp2_connection(conn: &mut NgServerConnection) {
    let deadline = tokio::time::Instant::now() + LINGER_TIMEOUT;
    while let Ok(Ok(_)) = tokio::time::timeout_at(deadline, conn.next_event()).await {}
}

/// tokio-http2 サーバーを起動し、GET 1 件に応答する
///
/// 受信した `:method` が GET であることを確認し、`:status=200` と
/// [`RESPONSE_BODY`] を返す。返り値はサーバーの待ち受けアドレスと応答処理タスクの
/// [`JoinHandle`] である。
///
/// # Panics
///
/// 関数本体は、証明書の生成と TLS 設定の構築、アドレスの解析、サーバーの bind に
/// 失敗した場合にパニックする。
/// 応答処理タスクは、接続の accept に失敗した場合と、accept が `IO_TIMEOUT` (5 秒) を
/// 超えた場合、イベントを 1 件受信するごとに `IO_TIMEOUT` (5 秒) を適用してその間に
/// イベントが届かない場合、イベントの受信と応答の送信に失敗した場合、
/// `:method` ヘッダーが無い場合、`:method` が GET でない場合にパニックする。
/// タスクのパニックは返り値の [`JoinHandle`] を `await` するまでテストへ伝播しない。
pub async fn serve_get_with_http2_server() -> (SocketAddr, JoinHandle<()>) {
    let server = Http2Server::bind(
        LISTEN_ADDR
            .parse()
            .expect("failed to parse the listen address"),
        http2_server_tls(),
        Limits::default(),
    )
    .await
    .expect("failed to bind the tokio-http2 server");
    let addr = server.local_addr();

    let handle = tokio::spawn(async move {
        let mut conn = tokio::time::timeout(IO_TIMEOUT, server.accept())
            .await
            .expect("timed out while waiting for a connection")
            .expect("failed to accept a connection");

        loop {
            let event = match tokio::time::timeout(IO_TIMEOUT, conn.next_event()).await {
                Ok(Ok(event)) => event,
                Ok(Err(e)) => panic!("failed to read an event from the peer: {e}"),
                Err(_) => panic!("timed out while waiting for a complete request"),
            };

            // GET のヘッダーを受信したら応答する
            let Http2Event::HeadersReceived {
                stream_id,
                headers,
                end_stream,
                ..
            } = event
            else {
                continue;
            };
            if !end_stream {
                continue;
            }

            let method = headers
                .iter()
                .find(|header| header.name() == b":method")
                .expect("the :method header is missing");
            assert_eq!(method.value(), b"GET", ":method が GET でない");

            let response_headers = vec![
                HeaderField::from_static(b":status", b"200"),
                HeaderField::from_static(b"content-type", b"text/plain"),
            ];
            conn.send_response(stream_id, response_headers, false)
                .await
                .expect("failed to send the response headers");
            conn.send_data(stream_id, RESPONSE_BODY.to_vec(), true)
                .await
                .expect("failed to send the response body");

            drain_http2_connection(&mut conn).await;
            break;
        }
    });

    (addr, handle)
}

/// tokio-nghttp2 サーバーを起動し、GET 1 件に応答する
///
/// 受信した `:method` が GET であることを確認し、`:status=200` と
/// [`RESPONSE_BODY`] を返す。返り値はサーバーの待ち受けアドレスと応答処理タスクの
/// [`JoinHandle`] である。
///
/// # Panics
///
/// 関数本体は、証明書の生成と TLS 設定の構築、アドレスの解析、サーバーの bind に
/// 失敗した場合にパニックする。
/// 応答処理タスクは、接続の accept に失敗した場合と、accept が `IO_TIMEOUT` (5 秒) を
/// 超えた場合、イベントを 1 件受信するごとに `IO_TIMEOUT` (5 秒) を適用してその間に
/// イベントが届かない場合、イベントの受信と応答の送信に失敗した場合、
/// `:method` ヘッダーが無い場合、`:method` が GET でない場合にパニックする。
/// タスクのパニックは返り値の [`JoinHandle`] を `await` するまでテストへ伝播しない。
pub async fn serve_get_with_nghttp2_server() -> (SocketAddr, JoinHandle<()>) {
    let server = NgServer::bind(
        LISTEN_ADDR
            .parse()
            .expect("failed to parse the listen address"),
        nghttp2_server_tls(),
    )
    .await
    .expect("failed to bind the tokio-nghttp2 server");
    let addr = server.local_addr();

    let handle = tokio::spawn(async move {
        let mut conn = tokio::time::timeout(IO_TIMEOUT, server.accept())
            .await
            .expect("timed out while waiting for a connection")
            .expect("failed to accept a connection");

        loop {
            let event = match tokio::time::timeout(IO_TIMEOUT, conn.next_event()).await {
                Ok(Ok(event)) => event,
                Ok(Err(e)) => panic!("failed to read an event from the peer: {e}"),
                Err(_) => panic!("timed out while waiting for a complete request"),
            };

            // GET のヘッダーを受信したら応答する
            let NgHttp2Event::HeadersReceived {
                stream_id,
                headers,
                end_stream,
                ..
            } = event
            else {
                continue;
            };
            if !end_stream {
                continue;
            }

            let method = headers
                .iter()
                .find(|header| header.name == b":method")
                .expect("the :method header is missing");
            assert_eq!(method.value, b"GET", ":method が GET でない");

            let response_headers = vec![
                NgHeader::status(200),
                NgHeader::new("content-type", "text/plain"),
            ];
            conn.send_response(stream_id, &response_headers, false)
                .await
                .expect("failed to send the response headers");
            conn.send_data(stream_id, RESPONSE_BODY, true)
                .await
                .expect("failed to send the response body");
            conn.flush().await.expect("failed to flush the response");

            drain_nghttp2_connection(&mut conn).await;
            break;
        }
    });

    (addr, handle)
}

/// tokio-http2 クライアントで GET を 1 往復する
///
/// `:status` と本文を返す。
///
/// # Panics
///
/// 接続の確立、リクエストの送信、応答の受信に失敗した場合と、応答が `IO_TIMEOUT`
/// (5 秒) 以内に届かない場合、ピアがストリームをリセットした場合、応答が完了する前に
/// ピアが GOAWAY を送った場合にパニックする。
/// 応答の `:status` が無い場合と、送信したストリーム ID と異なるイベントを
/// 受信した場合もパニックする。
pub async fn get_with_http2_client(addr: SocketAddr) -> Response {
    let mut client = tokio::time::timeout(
        IO_TIMEOUT,
        Http2Client::connect_insecure(addr, SERVER_NAME, Limits::default()),
    )
    .await
    .expect("timed out while connecting with the tokio-http2 client")
    .expect("failed to connect with the tokio-http2 client");

    // RFC 9113 Section 8.3.1: :method / :scheme / :path は必須。:authority は
    // authority 情報がある場合に使い、無い場合は送ってはならない
    let request_headers = vec![
        HeaderField::from_static(b":method", b"GET"),
        HeaderField::from_static(b":scheme", b"https"),
        HeaderField::from_static(b":path", b"/"),
        HeaderField::from_static(b":authority", SERVER_NAME.as_bytes()),
    ];
    let sent_stream_id = client
        .send_request(request_headers, true)
        .await
        .expect("failed to send a request");

    let mut status = Vec::new();
    let mut body = Vec::new();
    loop {
        let event = match tokio::time::timeout(IO_TIMEOUT, client.next_event()).await {
            Ok(Ok(event)) => event,
            Ok(Err(e)) => panic!("failed to read an event from the server: {e}"),
            Err(_) => panic!("timed out while waiting for a response from the server"),
        };

        match event {
            Http2Event::HeadersReceived {
                stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(
                    stream_id, sent_stream_id,
                    "異なるストリームのヘッダーを受信した"
                );
                let value = headers
                    .iter()
                    .find(|header| header.name() == b":status")
                    .expect("the :status header is missing");
                status = value.value().to_vec();

                // 本文を持たないレスポンスはここで完了する
                if end_stream {
                    break;
                }
            }
            Http2Event::DataReceived {
                stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(
                    stream_id, sent_stream_id,
                    "異なるストリームのデータを受信した"
                );
                body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            Http2Event::StreamReset {
                stream_id,
                error_code,
                ..
            } => panic!("the server reset the stream {stream_id:?}: {error_code:?}"),
            Http2Event::GoawayReceived { .. } => {
                panic!("the server sent GOAWAY before the response completed")
            }
            _ => {}
        }
    }

    client.shutdown().await.ok();
    Response { status, body }
}

/// tokio-nghttp2 クライアントで GET を 1 往復する
///
/// `:status` と本文を返す。ピアの SETTINGS の ACK はリクエスト送信の前提ではないため
/// 待たない (RFC 9113 Section 3.4。ACK の意味は Section 6.5.3)。
///
/// # Panics
///
/// 接続の確立、リクエストの送信、応答の受信に失敗した場合と、応答が `IO_TIMEOUT`
/// (5 秒) 以内に届かない場合、ピアがストリームを閉じた場合、応答が完了する前に
/// ピアが GOAWAY を送った場合にパニックする。
/// 応答の `:status` が無い場合と、送信したストリーム ID と異なるイベントを
/// 受信した場合もパニックする。
pub async fn get_with_nghttp2_client(addr: SocketAddr) -> Response {
    let mut client =
        tokio::time::timeout(IO_TIMEOUT, NgClient::connect_insecure(addr, SERVER_NAME))
            .await
            .expect("timed out while connecting with the tokio-nghttp2 client")
            .expect("failed to connect with the tokio-nghttp2 client");

    let request_headers = vec![
        NgHeader::method("GET"),
        NgHeader::scheme("https"),
        NgHeader::authority(SERVER_NAME),
        NgHeader::path("/"),
    ];
    let sent_stream_id = client
        .send_request(&request_headers, None, true)
        .await
        .expect("failed to send a request");
    client.flush().await.expect("failed to flush the request");

    let mut status = Vec::new();
    let mut body = Vec::new();
    loop {
        let event = match tokio::time::timeout(IO_TIMEOUT, client.next_event()).await {
            Ok(Ok(event)) => event,
            Ok(Err(e)) => panic!("failed to read an event from the server: {e}"),
            Err(_) => panic!("timed out while waiting for a response from the server"),
        };

        match event {
            NgHttp2Event::HeadersReceived {
                stream_id,
                headers,
                end_stream,
                ..
            } => {
                assert_eq!(
                    stream_id, sent_stream_id,
                    "異なるストリームのヘッダーを受信した"
                );
                let value = headers
                    .iter()
                    .find(|header| header.name == b":status")
                    .expect("the :status header is missing");
                status = value.value.to_vec();

                // 本文を持たないレスポンスはここで完了する
                if end_stream {
                    break;
                }
            }
            NgHttp2Event::DataReceived {
                stream_id,
                data,
                end_stream,
            } => {
                assert_eq!(
                    stream_id, sent_stream_id,
                    "異なるストリームのデータを受信した"
                );
                body.extend_from_slice(&data);
                if end_stream {
                    break;
                }
            }
            // nghttp2 は正常終了でも StreamClosed を発行するため、この分岐に到達するのは
            // 応答が完了する前にストリームが閉じた場合だけである
            NgHttp2Event::StreamClosed {
                stream_id,
                error_code,
            } => panic!(
                "the server closed the stream {stream_id:?} before the response completed: {error_code:?}"
            ),
            NgHttp2Event::GoawayReceived { .. } => {
                panic!("the server sent GOAWAY before the response completed")
            }
            _ => {}
        }
    }

    client.shutdown().await.ok();
    Response { status, body }
}
