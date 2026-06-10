# tokio-http2

[shiguredo_http2](https://github.com/shiguredo/http2-rs) (Sans I/O) を [Tokio](https://github.com/tokio-rs/tokio) と統合し、非同期 HTTP/2 クライアント/サーバーを提供するクレートです。

## 概要

tokio-http2 は Sans I/O な HTTP/2 実装 (shiguredo_http2) の上に、Tokio ベースの非同期 I/O 層を提供します。TLS には [Rustls](https://github.com/rustls/rustls) を、暗号ライブラリには [aws-lc-rs](https://github.com/aws/aws-lc-rs) を使用しています。

WebTransport over HTTP/2 (draft-ietf-webtrans-http2-14) のサーバー実装も `webtransport` モジュールで提供します。

## 依存ライブラリ

- [shiguredo_http2](https://github.com/shiguredo/http2-rs) - Sans I/O な HTTP/2 実装
- [Tokio](https://github.com/tokio-rs/tokio) - 非同期ランタイム
- [Rustls](https://github.com/rustls/rustls) - TLS 実装
- [aws-lc-rs](https://github.com/aws/aws-lc-rs) - 暗号ライブラリ
- [rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier) - プラットフォームの証明書検証

## クライアント

```rust
use tokio_http2::{Client, Event, HeaderField, Limits, TlsClientConfig};

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

### Client API

#### 接続

- `Client::connect()` - サーバーに接続
- `Client::connect_insecure()` - 証明書検証なしで接続 (テスト用)
- `Client::local_addr()` / `Client::remote_addr()` - 接続アドレスを取得

#### リクエスト送信

- `Client::send_request()` - リクエスト HEADERS を送信
- `Client::send_data()` - ストリームに DATA を追加送信
- `Client::send_trailers()` - END_STREAM 付きトレーラー HEADERS を送信

#### イベントループ

- `Client::poll_event()` - キューからイベントを取り出す
- `Client::next_event()` - イベントを待機
- `Client::drive()` - I/O とイベント処理を 1 回まわす
- `Client::recv()` - ネットワークから受信
- `Client::flush()` - 送信バッファをフラッシュ

#### コネクション制御

- `Client::ping()` - PING を送信
- `Client::send_window_update()` - WINDOW_UPDATE を送信
- `Client::shutdown()` - GOAWAY (NoError) を送信して接続を終了

## サーバー

```rust
use tokio_http2::{Event, HeaderField, Limits, Server, TlsServerConfig};

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
            conn.send_data(stream_id, b"Hello, HTTP/2!".to_vec(), true).await?;
        }
        _ => {}
    }
}
```

### Server / ServerConnection API

#### サーバー起動

- `Server::bind()` - アドレスにバインド
- `Server::accept()` - 接続を受け入れ
- `Server::local_addr()` - バインドアドレスを取得

#### レスポンス送信

- `ServerConnection::send_response()` - レスポンス HEADERS を送信
- `ServerConnection::send_data()` - DATA を送信
- `ServerConnection::send_trailers()` - END_STREAM 付きトレーラー HEADERS を送信

#### イベントループ

- `ServerConnection::poll_event()` - キューからイベントを取り出す
- `ServerConnection::next_event()` - イベントを待機
- `ServerConnection::drive()` - I/O とイベント処理を 1 回まわす
- `ServerConnection::recv()` - ネットワークから受信
- `ServerConnection::flush()` - 送信バッファをフラッシュ

#### コネクション制御

- `ServerConnection::reset_stream()` - RST_STREAM を送信
- `ServerConnection::send_window_update()` - WINDOW_UPDATE を送信
- `ServerConnection::shutdown()` - GOAWAY (NoError) を送信して接続を終了

## WebTransport

`webtransport` モジュールは draft-ietf-webtrans-http2-14 ベースの WebTransport over HTTP/2 サーバー実装を提供します。Extended CONNECT (`:protocol=webtransport`) で確立されたセッション上で、Capsule Protocol によって双方向 / 単方向ストリームと DATAGRAM を多重化します。

```rust
use shiguredo_http2::webtransport::WtConfig;
use tokio_http2::{Event, Server, TlsServerConfig};
use tokio_http2::webtransport::{WEBTRANSPORT_PROTOCOL, WtServerRequest};

let server = Server::bind(addr, tls_config, limits).await?;
let mut conn = server.accept().await?;

loop {
    match conn.next_event().await? {
        Event::HeadersReceived { stream_id, headers, end_stream, .. } => {
            // Extended CONNECT (`:method=CONNECT` + `:protocol=webtransport`) を判定
            let is_webtransport = headers.iter().any(|h| {
                h.name() == b":protocol" && h.value() == WEBTRANSPORT_PROTOCOL
            });
            if is_webtransport {
                let request = WtServerRequest::from_connection(conn, stream_id, headers);
                // 第 2 引数は Origin 検証用 (draft-ietf-webtrans-http2-14 Section 3.2)。
                // None で Origin 検証をスキップする。Web context では Some(b"https://...") を指定する
                let mut session = request.accept(WtConfig::default(), None).await?;

                // 双方向ストリームを受け入れる
                while let Some(mut bidi) = session.accept_bidi().await {
                    let data = bidi.recv().await?;
                    bidi.send(b"hello".to_vec(), true).await?;
                }
                break;
            }
        }
        _ => {}
    }
}
```

### 公開型

- `WtServerRequest` - 受信した Extended CONNECT 要求。 `accept()` で受諾、 `reject(status)` で拒否
- `WtServerSession` - 受諾後の WebTransport セッション
- `WtSessionHandle` / `WtSessionParts` - セッションを複数タスクで共有するためのハンドル
- `WtBidiStream` - 双方向ストリーム (`send` / `recv` / `stop_sending` / `reset`)
- `WtUniRecvStream` - 受信専用単方向ストリーム (`recv` / `stop_sending`)
- `WtUniSendStream` - 送信専用単方向ストリーム (`send` / `reset`)
- `WEBTRANSPORT_PROTOCOL` - `:protocol` 擬似ヘッダー値 (`b"webtransport"`) の定数

### `WtServerRequest`

- `from_connection()` - Extended CONNECT を受信済みの `ServerConnection` から要求を構築
- `stream_id()` / `headers()` / `path()` / `authority()` / `scheme()` / `origin()` - 要求情報を参照
- `webtransport_init()` - `WebTransport-Init` ヘッダー値 (RFC 8941 Dictionary) を取得
- `accept(config)` - セッションを受諾して `WtServerSession` を返す
- `reject(status)` - 指定ステータスで拒否

### `WtServerSession`

- `session_id()` - CONNECT ストリーム ID
- `accept_bidi()` / `accept_uni()` - ピアからのストリームを受け入れ
- `recv_datagram()` - DATAGRAM を受信
- `open_bidi()` / `open_uni()` - こちらからストリームを開く
- `send_datagram()` - DATAGRAM を送信
- `close()` - セッションを閉じる
- `drain()` - 送信中データの完了を待つ
- `into_parts()` - `WtSessionParts` に分解 (handle と receiver を独立タスクで扱う)

## 再エクスポート

- `Connection<S>` - 任意の `AsyncRead + AsyncWrite` ストリーム上に HTTP/2 を載せる低レベル型
- `shiguredo_http2` からの再エクスポート: `ErrorCode` / `Event` / `HeaderField` / `Limits` / `LimitsBuilder` / `StreamId`
- `CONNECTION_PREFACE` - HTTP/2 コネクションプリフェイス定数

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
