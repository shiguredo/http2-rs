# CONNECT ストリームと WtSession を接続する glue を実装する

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

`tokio-http2::ServerConnection` の内部で、Extended CONNECT ストリームに届いた DATA frame の payload を対応する `WtSession` に流し込み、`WtSession.poll_output()` の結果を同じストリームに DATA frame として送出するルーティング層を実装する。

## 背景

0003 で API の外形は用意されるが、実際のデータパス (HTTP/2 DATA ↔ Capsule) の橋渡しが無いと動作しない。
`WtSession` は Sans I/O のため、`feed(data)` → `process()` → `poll_event()` / `poll_output()` のサイクルを回す「駆動ループ」が必要になる。

## 根拠

- draft-ietf-webtrans-http2-14 Section 4: WebTransport セッションは CONNECT ストリーム内で Capsule Protocol によって多重化される
- `WtSession` は Capsule 単位で入出力するが、HTTP/2 は DATA frame 単位で届く。両者のバウンダリを吸収する必要がある

## 対応内容

### ServerConnection 内部状態

- `HashMap<StreamId, WtSession>` を保持
- `WtServerSession` を作るタイミングで `WtSession::server(config)` を生成し登録
- セッション終了時に登録解除

### 受信経路

- `poll_event()` が `DataReceived { stream_id, data, end_stream }` を返した時:
  - 登録済み WT ストリームなら `WtSession.feed(&data)` → `process()`
  - `WtSession.poll_event()` をドレインして `WtBidiStream` 等のチャネルへ push
  - end_stream の場合は CONNECT ストリームのクローズとしてセッション終了

### 送信経路

- `WtBidiStream::send(data)` などは `WtSession.send_stream_data(...)` を呼び、`WtSession.poll_output()` で得た Capsule データを `ServerConnection::send_data(stream_id, payload, false)` で送信
- flush はアプリ側で async 完了させる

### 同期

- `WtServerSession::into_parts()` で bidi_acceptor / uni_rx / session handle に分解し、`tokio::select!` で競合無く扱える構造にする (mpsc or watch で橋渡し)

### エラー処理

- WtSession 側のエラー (Capsule decode 失敗, flow control 違反等) を `ServerConnection` のエラーに昇格させ、必要なら GOAWAY を送信

## 完了条件

- 0003 で定義された `WtBidiStream::{send, recv}` が実際に動作する (ローカルの統合テストで bidi エコーが通る)
- `cargo test --workspace` が通る

## 依存

- 0002, 0003

## 解決方法

- `crates/tokio-http2/src/webtransport.rs` の `DriverState` でルーティング層を実装
- CONNECT ストリームへの `Event::DataReceived` 受信時に `WtSession::feed` → `process` を呼び、`poll_event()` でイベントをドレイン
- `StreamOpened` / `StreamData` / `StreamReset` を stream 単位の mpsc チャネルに分配 (`stream_channels: HashMap<WtStreamId, Sender>`)
- `DatagramReceived` は datagram_tx チャネルへ流す
- `WtSession::poll_output()` の結果を `ServerConnection::send_data` で CONNECT ストリームに送出する `flush_wt_output`
- `DriverCmd` を mpsc (unbounded) で driver に送信し、送信系 API (`send_stream_data`, `open_bidi`, `open_uni`, `send_datagram`, `reset_stream`, `stop_sending`, `close`, `drain`) を駆動
- `tokio::select! { biased; cmd, event }` でコマンド優先のイベントループを実装 (backpressure なしのシンプル構成)
- CONNECT ストリーム終了 (`StreamReset` / `StreamClosed` / `end_stream=true` の `DataReceived`) 検知時に driver を終了
