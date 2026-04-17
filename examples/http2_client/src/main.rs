//! HTTP/2 クライアントサンプル
//!
//! tokio-http2 を使用した HTTP/2 クライアントの実装例。
//!
//! # 実行方法
//!
//! まずサーバーを起動:
//! ```bash
//! cargo run -p http2_server
//! ```
//!
//! 別のターミナルでクライアントを実行:
//! ```bash
//! cargo run -p http2_client
//! # または URL を指定
//! cargo run -p http2_client -- https://localhost:8443/health
//! ```

use shiguredo_http11::uri::Uri;
use tokio_http2::{Client, Event, HeaderField, Limits};

const DEFAULT_URL: &str = "https://localhost:8443/";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_URL.to_string());

    let uri = Uri::parse(&url)?;

    let host = uri.host().ok_or("missing host")?;
    let port = uri.port().unwrap_or(443);
    let path = if uri.path().is_empty() {
        "/"
    } else {
        uri.path()
    };

    println!("Connecting to {host}:{port}{path}");

    let addr = format!("{host}:{port}").parse()?;
    let limits = Limits::default()
        .with_max_concurrent_streams(Some(100))
        .with_initial_window_size(65535);

    let mut client = Client::connect_insecure(addr, host, limits).await?;

    println!("TLS handshake completed");

    // リクエストを送信
    let headers = vec![
        HeaderField::from_str(":method", "GET"),
        HeaderField::from_str(":path", path),
        HeaderField::from_str(":scheme", "https"),
        HeaderField::from_str(":authority", &format!("{host}:{port}")),
        HeaderField::from_str("user-agent", "shiguredo-http2"),
        HeaderField::from_str("accept", "*/*"),
    ];

    let stream_id = client.send_request(headers, true).await?;

    println!("Started stream {stream_id}");

    let mut response_received = false;

    loop {
        let event = client.next_event().await?;

        match &event {
            Event::ConnectionPreface => {
                println!("Received connection preface");
            }
            Event::SettingsReceived { ack } => {
                println!("Received SETTINGS (ack={ack})");
            }
            Event::HeadersReceived {
                stream_id: sid,
                headers,
                end_stream,
                ..
            } => {
                println!("Stream {sid}: received headers (end_stream={end_stream})");

                if let Some(status) = find_header(headers, ":status") {
                    println!("Status: {status}");
                }

                for h in headers {
                    if !h.name.starts_with(b":") {
                        println!(
                            "  {}: {}",
                            String::from_utf8_lossy(&h.name),
                            String::from_utf8_lossy(&h.value)
                        );
                    }
                }

                if *end_stream && *sid == stream_id {
                    response_received = true;
                }
            }
            Event::DataReceived {
                stream_id: sid,
                data,
                end_stream,
            } => {
                println!("Stream {sid}: received {} bytes", data.len());

                if !data.is_empty() {
                    println!("--- Response Body ---");
                    print!("{}", String::from_utf8_lossy(data));
                    println!("--- End Body ---");
                }

                if *end_stream && *sid == stream_id {
                    response_received = true;
                }
            }
            Event::StreamClosed { stream_id: sid } => {
                println!("Stream {sid}: closed");
                if *sid == stream_id {
                    response_received = true;
                }
            }
            Event::GoawayReceived {
                last_stream_id,
                error_code,
                ..
            } => {
                println!("Received GOAWAY (last_stream={last_stream_id}, error={error_code})");
            }
            Event::WindowUpdateReceived {
                stream_id: sid,
                increment,
            } => {
                println!("Stream {sid}: WINDOW_UPDATE +{increment}");
            }
            _ => {}
        }

        if response_received {
            println!("Response complete");
            break;
        }
    }

    Ok(())
}

fn find_header(headers: &[HeaderField], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|h| h.name == name.as_bytes())
        .map(|h| String::from_utf8_lossy(&h.value).to_string())
}
