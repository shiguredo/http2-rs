# tokio-http2 に WebTransport サーバー API を追加する

- Created: 2026-04-17
- Model: Opus 4.7

## 概要

`crates/tokio-http2/` に `webtransport` モジュールを新設し、サーバーが Extended CONNECT を受理してから WebTransport セッションを扱うための高レベル API を追加する。

## 背景

現状の `ServerConnection` は `HeadersReceived` / `DataReceived` などの低レベルイベントのみを公開しており、アプリケーションが直接 Capsule Protocol を扱う必要がある。
これを隠蔽し、http3-rs の `WtSessionRequest` / `WtBiStream` 等と対になる API を提供する。

## 根拠

- サンプル (`examples/wt_server/`) を「お手本」として成立させるためには、アプリ側コードから Capsule の低レイヤを露出してはならない
- 異なるトランスポート (HTTP/2 vs HTTP/3) で類似 API 体系を提供することで、上位アプリが選択しやすくなる

## 対応内容

### 新規ファイル `crates/tokio-http2/src/webtransport.rs`

- `WtServerRequest`
  - `from_connection(conn, stream_id, headers) -> Self`
  - `path()`, `authority()`, `scheme()`, `origin()` の getter
  - `accept(self) -> Result<WtServerSession>` (200 レスポンス送信 + `WtSession` 生成)
  - `reject(self, status: u16) -> Result<()>` (指定ステータスで拒否)

- `WtServerSession`
  - `session_id() -> u64` (CONNECT の HTTP/2 stream id)
  - `accept_bidi() -> Result<Option<WtBidiStream>>`
  - `accept_uni() -> Result<Option<WtUniRecvStream>>`
  - `open_bidi() -> Result<WtBidiStream>`
  - `open_uni() -> Result<WtUniSendStream>`
  - `close(error_code, reason) -> Result<()>`
  - `drain() -> Result<()>`
  - `into_parts()` (bidi_acceptor / uni_rx / handle に分解)

- `WtBidiStream` / `WtUniRecvStream` / `WtUniSendStream`
  - `send(&[u8])` / `recv()` / `finish()` / `reset(error_code)` / `stop_sending(error_code)` (対応方向のみ)
  - `stream_id() -> u64`

### `crates/tokio-http2/src/lib.rs`

- `webtransport` モジュールを re-export
- `ServerConnection::accept_wt_request(&mut self) -> Result<WtServerRequest>` の追加

### 依存

- 0002 で公開された API (`Event::HeadersReceived.protocol`, `Connection::remote_settings` など) を利用

## 完了条件

- `WtServerRequest` / `WtServerSession` / `Wt*Stream` が document comments 付きで公開されている
- glue (0004) と組み合わせた時に bidi エコーが動作する
- `cargo fmt` / `cargo clippy -D warnings` が通る

## スコープ外 (別 issue)

- DATA frame の `WtSession` へのルーティング本体: 0004
- WT DATAGRAM: 0005
- 動的フロー制御: 0006

## 依存

- 0002
