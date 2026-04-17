//! WebTransport over HTTP/2 エコーサーバーサンプル (draft-ietf-webtrans-http2-14)
//!
//! tokio-http2 の `WtServerRequest` / `WtServerSession` を使ったお手本実装。
//! bidi / uni ストリームと WT DATAGRAM capsule をそのままエコーする。
//!
//! # 使い方
//!
//! ```bash
//! cd examples/wt_server
//! cargo run -- --listen 127.0.0.1:8443
//! ```

mod error;
mod tls;

use std::net::SocketAddr;

use shiguredo_http2::settings::WtInitialSettings;
use shiguredo_http2::webtransport::WtConfig;

use tokio_http2::{
    Event, Limits, Server, ServerConnection, WtBidiStream, WtServerRequest, WtSessionHandle,
    WtSessionParts, WtUniRecvStream,
};

use crate::error::Error;

const DEFAULT_LISTEN: &str = "127.0.0.1:8443";

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = parse_args();

    if let Err(e) = run_server(&args.listen, args.reject_connect).await {
        log::error!("server error: {e}");
    }
}

async fn run_server(listen: &str, reject_connect: bool) -> Result<(), Error> {
    let addr: SocketAddr = listen
        .parse()
        .map_err(|e| Error::Other(format!("invalid listen address: {e}")))?;

    let tls_config = tls::generate_tls_server()?;

    let wt_settings = WtInitialSettings {
        initial_max_data: Some(4 * 1024 * 1024),
        initial_max_stream_data_uni: Some(512 * 1024),
        initial_max_stream_data_bidi_local: Some(512 * 1024),
        initial_max_stream_data_bidi_remote: Some(512 * 1024),
        initial_max_streams_uni: Some(100),
        initial_max_streams_bidi: Some(100),
    };
    let limits = Limits::default()
        .with_max_concurrent_streams(Some(100))
        .with_enable_connect_protocol(true)
        .with_webtransport(wt_settings);

    let server = Server::bind(addr, tls_config, limits).await?;
    let local_addr = server.local_addr();

    log::info!("WebTransport (HTTP/2) server listening on https://{local_addr}");

    loop {
        tokio::select! {
            result = server.accept() => {
                match result {
                    Ok(conn) => {
                        let remote = conn.remote_addr();
                        log::info!("[{remote}] connection accepted");
                        let reject = reject_connect;
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(conn, reject).await {
                                log::error!("[{remote}] connection error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        log::error!("accept error: {e}");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                log::info!("shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_connection(mut conn: ServerConnection, reject_connect: bool) -> Result<(), Error> {
    let remote = conn.remote_addr();

    // Extended CONNECT 到着を待つ
    let (stream_id, headers) = loop {
        let ev = conn.next_event().await?;
        if let Event::HeadersReceived {
            stream_id,
            headers,
            end_stream,
            protocol,
        } = ev
        {
            if protocol.as_deref() != Some(tokio_http2::WEBTRANSPORT_PROTOCOL) {
                log::warn!(
                    "[{remote}] non-WebTransport request received (protocol={:?}); closing",
                    protocol
                        .as_deref()
                        .map(|p| String::from_utf8_lossy(p).into_owned())
                );
                return Ok(());
            }
            if end_stream {
                log::warn!("[{remote}] CONNECT with END_STREAM (invalid); closing");
                return Ok(());
            }
            break (stream_id, headers);
        }
    };

    let req = WtServerRequest::from_connection(conn, stream_id, headers);
    let path = req
        .path()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .unwrap_or_default();
    let authority = req
        .authority()
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .unwrap_or_default();
    log::info!("[{remote}] WT CONNECT authority={authority} path={path}");

    if reject_connect {
        log::warn!("[{remote}] rejecting with 404 (--reject-connect)");
        req.reject(404).await?;
        return Ok(());
    }

    let session = req.accept(WtConfig::default()).await?;
    log::info!(
        "[{remote}] session accepted (session_id={})",
        session.session_id()
    );

    let parts = session.into_parts();
    run_echo(parts, remote).await
}

async fn run_echo(parts: WtSessionParts, remote: SocketAddr) -> Result<(), Error> {
    let WtSessionParts {
        mut bidi_rx,
        mut uni_rx,
        mut datagram_rx,
        handle,
        driver,
        ..
    } = parts;

    loop {
        tokio::select! {
            biased;
            bidi = bidi_rx.recv() => {
                match bidi {
                    Some(bidi) => {
                        let r = remote;
                        tokio::spawn(async move {
                            if let Err(e) = handle_bidi(bidi, r).await {
                                log::error!("[{r}] bidi error: {e}");
                            }
                        });
                    }
                    None => break,
                }
            }
            uni = uni_rx.recv() => {
                match uni {
                    Some(uni) => {
                        let r = remote;
                        let h = handle.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_uni(uni, h, r).await {
                                log::error!("[{r}] uni error: {e}");
                            }
                        });
                    }
                    None => break,
                }
            }
            datagram = datagram_rx.recv() => {
                match datagram {
                    Some(data) => {
                        log::debug!("[{remote}] datagram received: {} bytes", data.len());
                        if let Err(e) = handle.send_datagram(data).await {
                            log::error!("[{remote}] datagram echo failed: {e}");
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    let _ = driver.await;
    log::info!("[{remote}] session finished");
    Ok(())
}

async fn handle_bidi(mut bidi: WtBidiStream, remote: SocketAddr) -> Result<(), Error> {
    let sid = bidi.stream_id();
    log::info!("[{remote}] bidi stream accepted (id={sid})");
    while let Some(data) = bidi.recv().await? {
        log::debug!("[{remote}] bidi {sid}: received {} bytes", data.len());
        bidi.send(data, false).await?;
    }
    log::info!("[{remote}] bidi stream closed (id={sid})");
    Ok(())
}

async fn handle_uni(
    mut uni: WtUniRecvStream,
    handle: WtSessionHandle,
    remote: SocketAddr,
) -> Result<(), Error> {
    let recv_id = uni.stream_id();
    log::info!("[{remote}] uni recv stream accepted (id={recv_id})");

    // エコー先の送信ストリームを開く
    let send_stream = handle.open_uni().await?;
    let send_id = send_stream.stream_id();
    log::info!("[{remote}] uni send stream opened (id={send_id}) for echo of {recv_id}");

    while let Some(data) = uni.recv().await? {
        log::debug!(
            "[{remote}] uni {recv_id}: received {} bytes; echoing on {send_id}",
            data.len()
        );
        send_stream.send(data, false).await?;
    }
    // FIN を送って送信側を閉じる
    send_stream.send(Vec::new(), true).await?;
    log::info!("[{remote}] uni streams finished (recv={recv_id}, send={send_id})");
    Ok(())
}

struct Args {
    listen: String,
    reject_connect: bool,
}

fn parse_args() -> Args {
    let mut args = noargs::raw_args();
    args.metadata_mut().app_name = "wt_server";
    args.metadata_mut().app_description = "WebTransport over HTTP/2 echo server";

    if noargs::VERSION_FLAG.take(&mut args).is_present() {
        println!("wt_server {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    noargs::HELP_FLAG.take_help(&mut args);

    let listen: String = noargs::opt("listen")
        .short('l')
        .ty("ADDR")
        .doc("Listen address")
        .default(DEFAULT_LISTEN)
        .take(&mut args)
        .then(|o| Ok::<_, std::convert::Infallible>(o.value().to_string()))
        .unwrap();

    let reject_connect: bool = noargs::flag("reject-connect")
        .doc("Reject every WebTransport CONNECT with 404 (WtServerRequest::reject demo)")
        .take(&mut args)
        .is_present();

    if let Ok(Some(help)) = args.finish() {
        print!("{help}");
        std::process::exit(0);
    }

    Args {
        listen,
        reject_connect,
    }
}
