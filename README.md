# http2-rs

[![shiguredo_http2](https://img.shields.io/crates/v/shiguredo_http2.svg)](https://crates.io/crates/shiguredo_http2)
[![Documentation](https://docs.rs/shiguredo_http2/badge.svg)](https://docs.rs/shiguredo_http2)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/shiguredo)

> [!WARNING]
> このライブラリは開発中であり、仕様が積極的に変更される場合があります。

## About Shiguredo's open source software

We will not respond to PRs or issues that have not been discussed on Discord. Also, Discord is only available in Japanese.

Please read <https://github.com/shiguredo/oss> before use.

## 時雨堂のオープンソースソフトウェアについて

利用前に <https://github.com/shiguredo/oss> をお読みください。

## 概要

Rust で実装された依存 0 かつ Sans I/O な HTTP/2 と WebTransport over HTTP/2 ライブラリです。

## 特徴

- Sans I/O
  - <https://sans-io.readthedocs.io/index.html>
- 依存ライブラリ 0
- RFC 9113 (HTTP/2) および RFC 7541 (HPACK) 準拠
- RFC 9218 (Extensible Priorities) サポート
- WebTransport over HTTP/2 (draft-ietf-webtrans-http2) サポート

## 使い方

### クライアント (リクエスト送信、レスポンス受信)

```rust
use shiguredo_http2::{Connection, HeaderField, Limits, Role};

// クライアント接続を生成
let limits = Limits::default()
    .with_max_concurrent_streams(Some(100))
    .with_initial_window_size(65535);
let mut conn = Connection::client(limits);

// 接続を開始 (プリフェイスと SETTINGS を送信)
conn.initiate()?;

// 送信データを取得
// output を送信...
// if let Some(output) = conn.poll_output() { ... }

// リクエストを送信
let headers = vec![
    HeaderField::from_str(":method", "GET"),
    HeaderField::from_str(":path", "/"),
    HeaderField::from_str(":scheme", "https"),
    HeaderField::from_str(":authority", "example.com"),
];
let stream_id = conn.start_stream(headers, true)?;

// 受信データを入力
// conn.feed(&received_data)?;
// conn.process()?;
// while let Some(event) = conn.poll_event() { ... }
```

### サーバー (リクエスト受信、レスポンス送信)

```rust
use shiguredo_http2::{Connection, HeaderField, Limits, Role};

// サーバー接続を生成
let limits = Limits::default();
let mut conn = Connection::server(limits);

// 接続を開始 (SETTINGS を送信)
conn.initiate()?;

// 受信データを入力
// conn.feed(&received_data)?;
// conn.process()?;

// イベントを処理してレスポンスを送信
// while let Some(event) = conn.poll_event() {
//     match event {
//         Event::HeadersReceived { stream_id, headers, end_stream } => {
//             let response_headers = vec![
//                 HeaderField::from_str(":status", "200"),
//                 HeaderField::from_str("content-type", "text/plain"),
//             ];
//             conn.send_response(stream_id, response_headers, false)?;
//             conn.send_data(stream_id, b"Hello, HTTP/2!".to_vec(), true)?;
//         }
//         _ => {}
//     }
// }
```

### HPACK エンコード/デコード

```rust
use shiguredo_http2::{HpackEncoder, HpackDecoder, HeaderField};

// エンコード
let mut encoder = HpackEncoder::new(4096);
let headers = vec![
    HeaderField::from_str(":status", "200"),
    HeaderField::from_str("content-type", "text/plain"),
];
let mut encoded = Vec::new();
encoder.encode(&mut encoded, &headers);

// デコード
let mut decoder = HpackDecoder::new(4096);
let decoded = decoder.decode(&encoded)?;
```

### WebTransport over HTTP/2

```rust
use shiguredo_http2::webtransport::{WtSession, WtConfig, WtEvent};

// クライアントセッションを生成
let mut session = WtSession::client(WtConfig::default());
session.initiate()?;

// 双方向ストリームを開く
let stream_id = session.open_bidi_stream()?;

// データを送信
session.send_stream_data(stream_id, b"Hello", false)?;

// データグラムを送信
session.send_datagram(b"Datagram")?;

// 受信データを入力
// session.feed(&received_data)?;
// session.process()?;
// while let Some(event) = session.poll_event() { ... }
```

## HTTP/2

このライブラリが対応している HTTP/2 の機能です。

### フレームタイプ (RFC 9113 Section 6)

- DATA - データ転送
- HEADERS - ヘッダー (HPACK 圧縮)
- PRIORITY - 優先度 (非推奨、受信のみ処理)
- RST_STREAM - ストリームリセット
- SETTINGS - 接続設定
- PING - 接続確認
- GOAWAY - 接続終了
- WINDOW_UPDATE - フロー制御
- CONTINUATION - ヘッダー継続
- PRIORITY_UPDATE - 優先度更新 (RFC 9218)

### SETTINGS パラメータ (RFC 9113 Section 6.5.2)

- SETTINGS_HEADER_TABLE_SIZE (0x01)
- SETTINGS_ENABLE_PUSH (0x02)
- SETTINGS_MAX_CONCURRENT_STREAMS (0x03)
- SETTINGS_INITIAL_WINDOW_SIZE (0x04)
- SETTINGS_MAX_FRAME_SIZE (0x05)
- SETTINGS_MAX_HEADER_LIST_SIZE (0x06)
- SETTINGS_ENABLE_CONNECT_PROTOCOL (0x08) - RFC 8441
- SETTINGS_NO_RFC7540_PRIORITIES (0x09) - RFC 9218

### HPACK (RFC 7541)

- 静的テーブル (61 エントリ)
- 動的テーブル (サイズ可変)
- Huffman エンコード/デコード
- 整数エンコード (5, 6, 7 ビットプレフィックス)

### フロー制御 (RFC 9113 Section 5.2)

- 接続レベルのフロー制御
- ストリームレベルのフロー制御
- WINDOW_UPDATE による送信ウィンドウ更新
- SETTINGS_INITIAL_WINDOW_SIZE による初期ウィンドウサイズ設定

### ストリーム状態遷移 (RFC 9113 Section 5.1)

- idle
- open
- half-closed (local)
- half-closed (remote)
- closed

### 非推奨機能

以下の機能は RFC 9113 で非推奨となっており、受信時のみ処理します:

- PRIORITY フレーム
- HEADERS フレームの優先度フィールド

### 未実装機能

以下の機能は実装していません:

- サーバープッシュ (PUSH_PROMISE)
  - Chrome/Firefox/Safari が削除済み
- WebSocket over HTTP/2
  - Extended CONNECT (RFC 8441) は実装済み

## WebTransport over HTTP/2

draft-ietf-webtrans-http2 で定義される WebTransport をサポートします。

### Capsule Protocol (RFC 9297)

- DATAGRAM
- PADDING

### WebTransport Capsule

- WT_STREAM
- WT_RESET_STREAM
- WT_STOP_SENDING
- WT_MAX_DATA
- WT_MAX_STREAM_DATA
- WT_MAX_STREAMS
- WT_DATA_BLOCKED
- WT_STREAM_DATA_BLOCKED
- WT_STREAMS_BLOCKED
- WT_CLOSE_SESSION
- WT_DRAIN_SESSION

## 制限 (DoS 対策)

デフォルト値:

- 最大同時ストリーム数: 100
- 初期ウィンドウサイズ: 65535 バイト
- 最大フレームサイズ: 16384 バイト
- 最大ヘッダーリストサイズ: 16384 バイト
- HPACK 動的テーブルサイズ: 4096 バイト

`Limits` で各制限値をカスタマイズ可能です。

## クレート構成

このリポジトリには以下のクレートが含まれています。

### shiguredo_http2

Sans I/O な HTTP/2 ライブラリ本体です。依存ライブラリ 0 で RFC 9113 に準拠しています。

### tokio-http2

shiguredo_http2 (Sans I/O) を [Tokio](https://github.com/tokio-rs/tokio) と統合し、非同期 HTTP/2 クライアント/サーバーを提供します。

TLS には [Rustls](https://github.com/rustls/rustls) を、暗号ライブラリには [aws-lc-rs](https://github.com/aws/aws-lc-rs) を使用しています。

### nghttp2-sys

[nghttp2](https://nghttp2.org/) C ライブラリへの低レベル FFI バインディングです。ビルド時に nghttp2 のソースコードを取得し、cmake でビルドします。

### shiguredo_nghttp2

nghttp2 の Rust バインディングです。nghttp2-sys の上に安全な Rust API を提供します。

### tokio-nghttp2

shiguredo_nghttp2 を [Tokio](https://github.com/tokio-rs/tokio) と統合し、非同期 HTTP/2 クライアント/サーバーを提供します。

TLS には [Rustls](https://github.com/rustls/rustls) を、暗号ライブラリには [aws-lc-rs](https://github.com/aws/aws-lc-rs) を使用しています。

tokio-http2 との差異は、HTTP/2 プロトコル処理に nghttp2 を使用している点です。nghttp2 は HTTP/2 のフレーム処理、HPACK、フロー制御を担当し、TLS は Rustls で処理します。

## サンプル

サンプルは [tokio-http2](crates/tokio-http2) クレートを使用しています。

### http2_client

HTTP/2 クライアントの例です。

```bash
# まずサーバーを起動
cargo run -p http2_server

# 別のターミナルでクライアントを実行
cargo run -p http2_client
# または URL を指定
cargo run -p http2_client -- https://localhost:8443/health
```

### http2_server

HTTP/2 サーバーの例です。自己署名証明書を自動生成します。

```bash
cargo run -p http2_server
```

テスト方法:

```bash
cargo run -p http2_client
# または
curl -k --http2 https://localhost:8443/
```

## 規格書

このライブラリが準拠している RFC 一覧です。

- RFC 7541 - HPACK: Header Compression for HTTP/2
  - <https://datatracker.ietf.org/doc/html/rfc7541>
- RFC 9113 - HTTP/2
  - <https://datatracker.ietf.org/doc/html/rfc9113>
- RFC 9218 - Extensible Prioritization Scheme for HTTP
  - <https://datatracker.ietf.org/doc/html/rfc9218>
- RFC 9297 - HTTP Datagrams and the Capsule Protocol
  - <https://datatracker.ietf.org/doc/html/rfc9297>
- draft-ietf-webtrans-http2 - WebTransport over HTTP/2
  - <https://datatracker.ietf.org/doc/html/draft-ietf-webtrans-http2>

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
