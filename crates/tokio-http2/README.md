# tokio-http2

[shiguredo_http2](https://github.com/shiguredo/http2-rs) (Sans I/O) を [Tokio](https://github.com/tokio-rs/tokio) と統合し、非同期 HTTP/2 クライアント/サーバーを提供するクレートです。

## 概要

tokio-http2 は Sans I/O な HTTP/2 実装 (shiguredo_http2) の上に、Tokio ベースの非同期 I/O 層を提供します。TLS には [Rustls](https://github.com/rustls/rustls) を、暗号ライブラリには [aws-lc-rs](https://github.com/aws/aws-lc-rs) を使用しています。

## 依存ライブラリ

- [shiguredo_http2](https://github.com/shiguredo/http2-rs) - Sans I/O な HTTP/2 実装
- [Tokio](https://github.com/tokio-rs/tokio) - 非同期ランタイム
- [Rustls](https://github.com/rustls/rustls) - TLS 実装
- [aws-lc-rs](https://github.com/aws/aws-lc-rs) - 暗号ライブラリ
- [rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier) - プラットフォームの証明書検証

## クライアント

```rust
use tokio_http2::{Client, HeaderField, Limits, TlsClientConfig};

let limits = Limits::default();
let tls_config = TlsClientConfig::with_platform_verifier()?;

let mut client = Client::connect(addr, "example.com", tls_config, limits).await?;

// リクエスト送信
let headers = vec![
    HeaderField::from_str(":method", "GET"),
    HeaderField::from_str(":scheme", "https"),
    HeaderField::from_str(":path", "/"),
    HeaderField::from_str(":authority", "example.com"),
];
let stream_id = client.send_request(headers, true).await?;

// レスポンス受信
loop {
    match client.next_event().await? {
        Event::HeadersReceived { stream_id, headers, end_stream, .. } => {
            // ヘッダー処理
        }
        Event::DataReceived { stream_id, data, end_stream } => {
            // データ処理
        }
        _ => {}
    }
}
```

## サーバー

```rust
use tokio_http2::{Server, HeaderField, Limits, TlsServerConfig};

let tls_config = TlsServerConfig::new(cert_pem, key_pem)?;
let limits = Limits::default();

let server = Server::bind(addr, tls_config, limits).await?;
let mut conn = server.accept().await?;

loop {
    match conn.next_event().await? {
        Event::HeadersReceived { stream_id, headers, end_stream, .. } => {
            // レスポンスヘッダー送信
            let response_headers = vec![
                HeaderField::from_str(":status", "200"),
                HeaderField::from_str("content-type", "text/plain"),
            ];
            conn.send_response(stream_id, response_headers, false).await?;

            // レスポンスボディ送信
            conn.send_data(
                stream_id,
                bytes::Bytes::from_static(b"Hello, HTTP/2!"),
                true,
            ).await?;
        }
        _ => {}
    }
}
```

## TLS 設定

### クライアント

```rust
// プラットフォームの証明書検証器を使用
let tls_config = TlsClientConfig::with_platform_verifier()?;

// カスタム CA 証明書を使用
let tls_config = TlsClientConfig::with_custom_ca(ca_cert_pem)?;

// 証明書検証なし (テスト用)
let tls_config = TlsClientConfig::insecure()?;
```

### サーバー

```rust
// PEM 形式の証明書と秘密鍵から作成
let tls_config = TlsServerConfig::new(cert_pem, key_pem)?;

// DER 形式から作成
let tls_config = TlsServerConfig::from_der(certs, key)?;
```

## ライセンス

Apache License 2.0

```text
Copyright 2026-2026, Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
