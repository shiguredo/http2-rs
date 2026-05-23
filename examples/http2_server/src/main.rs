//! HTTP/2 サーバーサンプル
//!
//! tokio-http2 を使用した HTTP/2 サーバーの実装例。
//!
//! # 実行方法
//!
//! ```bash
//! cargo run -p http2_server
//! ```
//!
//! # テスト方法
//!
//! ```bash
//! cargo run -p http2_client
//! # または
//! curl -k --http2 https://localhost:8443/
//! ```

use std::net::SocketAddr;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use tokio_http2::{Event, HeaderField, Limits, Server, ServerConnection, TlsServerConfig};

const LISTEN_ADDR: &str = "127.0.0.1:8443";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tls_config = create_tls_config()?;
    let addr: SocketAddr = LISTEN_ADDR.parse()?;

    let limits = Limits::default()
        .with_max_concurrent_streams(Some(100))
        .with_initial_window_size(65535);

    let server = Server::bind(addr, tls_config, limits).await?;

    println!("HTTP/2 server listening on https://{LISTEN_ADDR}");

    loop {
        match server.accept().await {
            Ok(conn) => {
                let peer_addr = conn.remote_addr();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(conn, peer_addr).await {
                        eprintln!("[{peer_addr}] error: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("accept error: {e}");
            }
        }
    }
}

fn create_tls_config() -> Result<TlsServerConfig, Box<dyn std::error::Error>> {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_string()])?;

    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        signing_key.serialize_der().to_vec(),
    ));

    Ok(TlsServerConfig::from_der(vec![cert_der], key_der)?)
}

async fn handle_connection(
    mut conn: ServerConnection,
    peer_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("[{peer_addr}] connected");

    loop {
        let event = match conn.next_event().await {
            Ok(event) => event,
            Err(tokio_http2::Error::ConnectionClosed) => {
                println!("[{peer_addr}] connection closed");
                break;
            }
            Err(e) => return Err(e.into()),
        };

        match &event {
            Event::ConnectionPreface => {
                println!("[{peer_addr}] received connection preface");
            }
            Event::SettingsReceived { ack } => {
                println!("[{peer_addr}] received SETTINGS (ack={ack})");
            }
            Event::HeadersReceived {
                stream_id,
                headers,
                end_stream,
                ..
            } => {
                println!(
                    "[{peer_addr}] stream {stream_id}: received headers (end_stream={end_stream})"
                );

                let method = find_header(headers, ":method").unwrap_or_else(|| "-".to_string());
                let path = find_header(headers, ":path").unwrap_or_else(|| "/".to_string());
                let scheme = find_header(headers, ":scheme").unwrap_or_else(|| "-".to_string());
                let authority =
                    find_header(headers, ":authority").unwrap_or_else(|| "-".to_string());

                println!("[{peer_addr}] stream {stream_id}: {method} {scheme}://{authority}{path}");

                if *end_stream {
                    send_response(&mut conn, *stream_id, &path).await?;
                }
            }
            Event::DataReceived {
                stream_id,
                data,
                end_stream,
            } => {
                println!(
                    "[{peer_addr}] stream {stream_id}: received {} bytes (end_stream={end_stream})",
                    data.len()
                );

                if *end_stream {
                    send_response(&mut conn, *stream_id, "/").await?;
                }
            }
            Event::StreamClosed { stream_id } => {
                println!("[{peer_addr}] stream {stream_id}: closed");
            }
            Event::GoawayReceived {
                last_stream_id,
                error_code,
                ..
            } => {
                println!(
                    "[{peer_addr}] received GOAWAY (last_stream={last_stream_id}, error={error_code})"
                );
                break;
            }
            Event::PingReceived { ack, .. } => {
                println!("[{peer_addr}] received PING (ack={ack})");
            }
            Event::WindowUpdateReceived {
                stream_id,
                increment,
            } => {
                println!("[{peer_addr}] stream {stream_id}: WINDOW_UPDATE +{increment}");
            }
            _ => {}
        }
    }

    Ok(())
}

fn find_header(headers: &[HeaderField], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|h| h.name() == name.as_bytes())
        .map(|h| String::from_utf8_lossy(h.value()).to_string())
}

async fn send_response(
    conn: &mut ServerConnection,
    stream_id: u32,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (status, body) = match path {
        "/" => ("200", "Hello, HTTP/2!\n"),
        "/health" => ("200", "OK\n"),
        _ => ("404", "Not Found\n"),
    };

    // 固定リテラルは from_static でコンパイル時検査、動的値は new でランタイム検査する。
    let headers = vec![
        HeaderField::new(":status", status).unwrap(),
        HeaderField::from_static(b"content-type", b"text/plain; charset=utf-8"),
        HeaderField::new("content-length", body.len().to_string()).unwrap(),
        HeaderField::from_static(b"server", b"shiguredo-http2"),
    ];

    conn.send_response(stream_id, headers, false).await?;
    conn.send_data(stream_id, body.as_bytes().to_vec(), true)
        .await?;

    Ok(())
}
