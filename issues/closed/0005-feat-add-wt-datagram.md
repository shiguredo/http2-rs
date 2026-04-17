# WT DATAGRAM capsule の送受信 API を実装する

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

`WtServerSession::send_datagram(&[u8])` と `WtServerSession::recv_datagram() -> Option<Vec<u8>>` を実装する。
HTTP/2 には QUIC のような DATAGRAM フレームは無いため、draft-ietf-webtrans-http2-14 に従い CONNECT ストリーム上の `DATAGRAM` capsule (type=0x00) で送受信する。

## 背景

`Capsule::Datagram { data }` は既に encode/decode 対応済み (`src/webtransport/capsule.rs`) で、`WtSession.send_datagram()` / `WtSession.poll_event()` → `WtEvent::DatagramReceived` も実装済み。
tokio-http2 の高レベル API として expose するだけで良い。

## 根拠

- draft-ietf-webtrans-http2-14 Section 6 / RFC 9297 Section 3.5: WebTransport over HTTP/2 の DATAGRAM は capsule として運ばれる
- HTTP/2 は TCP 上のため DATAGRAM も信頼・順序保証付きで届くが、意味論としては可変長メッセージとしてそのまま扱う

## 対応内容

### API

- `WtServerSession::send_datagram(&[u8]) -> Result<()>`
  - 内部で `WtSession::send_datagram` を呼び、`poll_output()` で得た Capsule を DATA frame で送出

- `WtServerSession::recv_datagram() -> Result<Vec<u8>>`
  - 0004 の glue で `WtEvent::DatagramReceived` を mpsc に流し、ここで await する
  - もしくは `DatagramStream` のようにハンドルを分離する (http3-rs 流)

### 呼び出し例

```rust
let datagram = session.recv_datagram().await?;
session.send_datagram(&datagram).await?;
```

## 完了条件

- エコーサンプル (0008) で DATAGRAM エコーが動作する
- 統合テスト (0007) で DATAGRAM のラウンドトリップが通る

## 依存

- 0002, 0003, 0004

## 解決方法

- `WtServerSession::send_datagram(Vec<u8>) -> Result<()>` を実装 (`DriverCmd::SendDatagram` 経由で `WtSession::send_datagram`)
- `WtServerSession::recv_datagram() -> Option<Vec<u8>>` を実装 (datagram_rx チャネルを await)
- `WtEvent::DatagramReceived` を driver で datagram_tx チャネルに push
- 実装は 0003 / 0004 と同一コミットに含めた
