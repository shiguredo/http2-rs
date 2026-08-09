---
name: shiguredo-http2
description: 時雨堂の Sans I/O HTTP/2 ライブラリ shiguredo_http2 と関連クレートの機能・API リファレンス。HTTP/2 フレーム処理、HPACK 圧縮、フロー制御、ストリーム管理、WebTransport over HTTP/2、Tokio 統合、nghttp2 バインディングに関する質問時に使用。
---

# shiguredo_http2

Sans I/O 設計に基づく HTTP/2 と WebTransport over HTTP/2 のライブラリ。

## 特徴

- **依存なし**: 外部依存ゼロ (`std` のみ)
- **Sans I/O**: I/O を完全に分離した設計 (Tokio, async-std, 同期 I/O など任意の環境で使用可能)
- **RFC 準拠**: RFC 9113 (HTTP/2), RFC 7541 (HPACK), RFC 9218 (Extensible Priorities) に対応
- **WebTransport over HTTP/2**: draft-ietf-webtrans-http2-15 に対応
- **HPACK**: 静的/動的テーブル、Huffman 符号化、サイズ更新対応
- **フロー制御**: 接続レベル/ストリームレベル両対応
- **DoS 対策**: `Limits` による各種上限設定

## バージョン情報

- crate 名: `shiguredo_http2`
- バージョン: 2026.1.0-canary.8
- Rust Edition: 2024
- 最小 Rust バージョン: 1.93
- ライセンス: Apache-2.0

## クレート構成

| クレート | 説明 |
|---------|------|
| `shiguredo_http2` | ルートクレート。Sans I/O な HTTP/2 と WebTransport over HTTP/2 の本体実装 |
| `nghttp2-sys` | nghttp2 C ライブラリへの低レベル FFI バインディング (ビルド時に `cmake` で静的リンク) |
| `shiguredo_nghttp2` | nghttp2 の Rust バインディング (`Session` を中心とした安全な API) |
| `tokio-http2` | shiguredo_http2 を Tokio + Rustls (aws-lc-rs) で非同期化。`Client` / `Server` / `WtServerSession` を提供 |
| `tokio-nghttp2` | shiguredo_nghttp2 を Tokio + Rustls (aws-lc-rs) で非同期化。`Client` / `Server` を提供 |

## コア API (shiguredo_http2)

### Connection

`Connection` は HTTP/2 接続の状態機械を表す。Sans I/O なので I/O は呼び出し側が行う。

| メソッド | 戻り値 | 説明 |
|---------|--------|------|
| `Connection::new(Role, Limits)` | `Self` | 新規接続を作成 (役割と制限を指定) |
| `Connection::client(Limits)` | `Self` | クライアント接続を作成 |
| `Connection::server(Limits)` | `Self` | サーバー接続を作成 |
| `role()` | `Role` | 役割 (`Client` / `Server`) を返す |
| `state()` | `ConnectionState` | 接続状態を返す |
| `is_active()` / `is_closed()` | `bool` | 状態判定 |
| `local_settings()` / `remote_settings()` | `&Settings` | 自分側 / 相手側の SETTINGS 値を取得 |
| `initiate()` | `Result<()>` | プリフェイスと初期 SETTINGS をキューに積む |
| `send_settings()` | `Result<()>` | SETTINGS を送信 |
| `mark_preface_sent()` / `mark_preface_received()` | `()` | プリフェイスの送受信完了をマーク (I/O 層からの通知用) |
| `feed(&[u8])` | `Result<usize>` | 受信バイトを内部バッファに投入し、消費バイト数を返す |
| `process()` | `Result<()>` | 内部バッファを処理してイベントを発生させる |
| `poll_event()` | `Option<Event>` | 発生したイベントを 1 つ取り出す |
| `poll_output()` | `Option<Vec<u8>>` | 送信すべきバイト列を 1 つ取り出す |
| `has_output()` | `bool` | 送信待ちがあるか判定 |
| `start_stream(Vec<HeaderField>, end_stream)` | `Result<StreamId>` | リクエストを送信し新しい `StreamId` を返す (クライアント用) |
| `send_response(StreamId, Vec<HeaderField>, end_stream)` | `Result<()>` | レスポンスヘッダーを送信 (サーバー用) |
| `send_data(StreamId, Vec<u8>, end_stream)` | `Result<()>` | DATA フレームを送信 |
| `send_trailers(StreamId, Vec<HeaderField>)` | `Result<()>` | トレーラーを END_STREAM 付きで送信 |
| `reset_stream(StreamId, ErrorCode)` | `Result<()>` | RST_STREAM を送信 |
| `send_ping([u8; 8])` | `Result<()>` | PING を送信 |
| `send_goaway(ErrorCode, Vec<u8>)` | `Result<()>` | GOAWAY を送信 |
| `send_window_update(StreamId, increment: u32)` | `Result<()>` | WINDOW_UPDATE を送信 |

#### Role / ConnectionState

| 型 | バリアント |
|----|-----------|
| `Role` | `Client`, `Server` |
| `ConnectionState` | `Idle`, `Open`, `Closing`, `Closed` |

### Event (Sans I/O イベント)

`Connection::poll_event()` で取得する HTTP/2 イベント。

| バリアント | フィールド |
|-----------|-----------|
| `ConnectionPreface` | (なし) クライアントから接続プリフェイスを受信した |
| `SettingsReceived` | `ack: bool` |
| `HeadersReceived` | `stream_id`, `headers: Vec<HeaderField>`, `end_stream: bool`, `protocol: Option<Vec<u8>>` (Extended CONNECT の `:protocol` 値、WebTransport では `Some(b"webtransport")`) |
| `DataReceived` | `stream_id`, `data: Vec<u8>`, `end_stream: bool` |
| `TrailersReceived` | `stream_id`, `trailers: Vec<HeaderField>` |
| `StreamReset` | `stream_id`, `error_code: ErrorCode` |
| `StreamClosed` | `stream_id` |
| `PingReceived` | `opaque_data: [u8; 8]`, `ack: bool` |
| `GoawayReceived` | `last_stream_id`, `error_code`, `debug_data: Vec<u8>` |
| `WindowUpdateReceived` | `stream_id`, `increment: u32` (`stream_id == 0` で接続レベル) |
| `PriorityUpdateReceived` | `stream_id`, `priority_field_value: Vec<u8>` (RFC 9218) |
| `ConnectionError` | `error_code`, `reason: String` |

`Event::stream_id() -> Option<StreamId>` と `Event::is_connection_level() -> bool` で分類できる。

### HPACK

| 型 | 説明 | 主要メソッド |
|----|------|-------------|
| `HpackEncoder` | HPACK エンコーダー (`hpack::Encoder` のエイリアス) | `new(max_table_size: usize)`, `set_huffman(bool)`, `set_max_table_size(usize)`, `encode(&mut Vec<u8>, &[HeaderField])`, `encode_header(&mut Vec<u8>, name, value, indexing)`, `encode_header_sensitive(&mut Vec<u8>, name, value)`, `encode_size_update(&mut Vec<u8>, new_size)`, `dynamic_table()` |
| `HpackDecoder` | HPACK デコーダー (`hpack::Decoder` のエイリアス) | `new(max_table_size: usize)`, `set_max_header_list_size(Option<usize>)`, `set_max_table_size(usize)`, `decode(&[u8]) -> Result<Vec<HeaderField>>`, `dynamic_table()` |
| `HeaderField` | HTTP/2 ヘッダーフィールド (HPACK 用、CRLF/NUL を構築時バリデーション) | `new(name, value) -> Result<Self, HeaderFieldError>`, `new_with_sensitive(name, value, sensitive) -> Result<Self, HeaderFieldError>`, `from_static(name, value) -> Self` (`const fn`、コンパイル時検査), `name() -> &[u8]`, `value() -> &[u8]`, `sensitive() -> bool` |
| `HeaderFieldError` | `HeaderField` の構築エラー | 不正なヘッダー名・値のバリデーション失敗 |

### Limits / Settings

| 型 | 説明 |
|----|------|
| `Limits` | HTTP/2 接続の上限設定 (`max_concurrent_streams`, `initial_window_size`, `max_frame_size`, `max_header_list_size`, `header_table_size`, `connection_window_size`, `enable_connect_protocol`, `no_rfc7540_priorities`, `wt_enabled`, `wt_initial_max_*`) |
| `LimitsBuilder` | `Limits` のビルダー。`builder()` で取得し、各メソッドで設定後 `build()` / `build_static()` (`const fn`)。WebTransport には `wt_enabled(true)` + `enable_connect_protocol(true)` + `webtransport(...)` が必要 (二重ゲート) |
| `LimitsError` | `Limits` 構築時のエラー (`WebtransportRequiresConnectProtocol`, `WebtransportRequiresWtEnabled`) |
| `Settings` | 受信した SETTINGS パラメータの値ホルダー (フィールド private、getter 経由) |
| `Setting` | 個別の SETTINGS パラメータ (`HeaderTableSize`, `EnablePush`, `MaxConcurrentStreams`, `InitialWindowSize`, `MaxFrameSize`, `MaxHeaderListSize`, `EnableConnectProtocol`, `NoRfc7540Priorities`, `WtEnabled`, `WtInitialMaxData`, `WtInitialMaxStreamDataUni`, `WtInitialMaxStreamDataBidiLocal`, `WtInitialMaxStreamsUni`, `WtInitialMaxStreamsBidi`, `WtInitialMaxStreamDataBidiRemote`, `Unknown { id, value }`) |
| `SettingError` | SETTINGS の値検証エラー (`EnablePushNotBoolean`, `InitialWindowSizeOutOfRange`, `MaxFrameSizeOutOfRange`, `EnableConnectProtocolNotBoolean`, `NoRfc7540PrioritiesNotBoolean`, `WtEnabledNotBoolean`) |
| `WindowSize` | フロー制御ウィンドウサイズ (`from_static(u32)` const + 動的 `new(u32) -> Result`) |
| `MaxFrameSize` | フレームサイズ上限 (16384..=16777215, RFC 9113 §6.5.2) |

**Limits のデフォルト値** (`Limits::default()` / `Limits::builder()`):

| 項目 | 値 |
|------|----|
| `max_concurrent_streams` | `Some(100)` |
| `initial_window_size` | 65535 (RFC 9113 §6.9.2) |
| `max_frame_size` | 16384 (RFC 9113 §6.5.2) |
| `max_header_list_size` | `Some(16384)` |
| `header_table_size` | 4096 (RFC 7541) |
| `enable_connect_protocol` | `false` |
| `wt_enabled` | `false` |
| `wt_initial_max_*` | すべて `None` |

### Stream / StreamId

| 型 | 説明 |
|----|------|
| `StreamId` | ストリーム ID。`Connection` バリアントは 0、それ以外は非ゼロ。クライアント発行は奇数、サーバー発行は偶数 |
| `NonZeroStreamId` | 非ゼロのストリーム ID (1..=2^31-1) |
| `ClientStreamId` / `ServerStreamId` | パリティ付きストリーム ID |
| `Parity` | `Odd` (Client) / `Even` (Server) |
| `StreamIdError` | ストリーム ID 構築時のエラー |
| `Stream` | ストリームの状態ホルダー |
| `StreamState` | RFC 9113 §5.1 のストリーム状態 (`Idle`, `Open`, `HalfClosedLocal`, `HalfClosedRemote`, `Closed`, etc.) |

### フロー制御

| 型 | 説明 |
|----|------|
| `FlowControl` | フロー制御ウィンドウ |
| `MAX_WINDOW_SIZE` | 最大ウィンドウサイズ (`2^31 - 1`) |

## フレーム型

| 型 | 説明 |
|----|------|
| `Frame` | 全フレーム型を統合する enum (`Data`, `Headers`, `Priority`, `RstStream`, `Settings`, `PushPromise`, `Ping`, `Goaway`, `WindowUpdate`, `Continuation`, `PriorityUpdate`, `Unknown`) |
| `FrameType` | フレーム種別 (`Data` = 0x00, `Headers` = 0x01, `Priority` = 0x02, `RstStream` = 0x03, `Settings` = 0x04, `PushPromise` = 0x05, `Ping` = 0x06, `Goaway` = 0x07, `WindowUpdate` = 0x08, `Continuation` = 0x09, `PriorityUpdate` = 0x10) |
| `FrameHeader` | 9 バイトフレームヘッダー (length, type, flags, stream_id) |
| `FrameFlags` | フレームフラグ (`END_STREAM`, `END_HEADERS`, `PADDED`, `PRIORITY`, `ACK`) |
| `FRAME_HEADER_SIZE` | フレームヘッダーサイズ定数 (9) |
| `DataFrame` / `HeadersFrame` / `RstStreamFrame` / `SettingsFrame` / `PingFrame` / `GoawayFrame` / `WindowUpdateFrame` / `ContinuationFrame` / `PriorityUpdateFrame` | 各種フレーム構造体 |
| `LastStreamId` | GOAWAY の `last_stream_id` |
| `Weight` | PRIORITY の重み (RFC 9113 では非推奨、受信のみ) |
| `WindowIncrement` | WINDOW_UPDATE の増分値 (1..=2^31-1) |
| `FrameDecoder` / `FrameEncoder` | 低レベルなフレームのデコーダー/エンコーダー (`Connection` の内部で使用) |
| `FrameError` | フレーム処理エラー |

**`CONNECTION_PREFACE`**: `b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"` (24 バイト)
**`CONNECTION_PREFACE_LEN`**: 24

## WebTransport over HTTP/2

`webtransport` モジュール。Sans I/O の `WtSession` を中心に、Capsule Protocol (RFC 9297) と WebTransport Capsule (draft-ietf-webtrans-http2-15) を扱う。

### WtSession

| メソッド | 戻り値 | 説明 |
|---------|--------|------|
| `WtSession::client(WtConfig, WtConfig)` / `WtSession::server(WtConfig, WtConfig)` | `Self` | セッションを作成。第 1 引数はローカル広告値、第 2 引数はピア広告値 (send_max / recv_max の初期化に使う) |
| `role()` / `state()` / `is_active()` / `is_closed()` | (各種) | セッション状態を返す |
| `initiate()` | `WtResult<()>` | 初期 SETTINGS を送信し `Active` に遷移 |
| `feed(&[u8])` | `WtResult<usize>` | 受信バイトを投入 |
| `process()` | `WtResult<()>` | 内部状態を進める |
| `poll_output()` | `Option<Vec<u8>>` | 送信すべきバイト列を取得 |
| `poll_event()` | `Option<WtEvent>` | イベントを取得 |
| `has_output()` | `bool` | 送信待ちがあるか |
| `open_bidi_stream()` / `open_uni_stream()` | `WtResult<WtStreamId>` | ストリームを開く |
| `send_stream_data(WtStreamId, &[u8], fin: bool)` | `WtResult<()>` | ストリームに WT_STREAM Capsule でデータ送信 |
| `reset_stream(WtStreamId, error_code: u64)` | `WtResult<()>` | WT_RESET_STREAM を送信 |
| `stop_sending(WtStreamId, error_code: u64)` | `WtResult<()>` | WT_STOP_SENDING を送信 |
| `send_datagram(&[u8])` | `WtResult<()>` | DATAGRAM Capsule (RFC 9297) を送信 |
| `close(error_code: u32, reason: &str)` | `WtResult<()>` | WT_CLOSE_SESSION を送信してセッション終了。reason が 1024 バイト超過時は UTF-8 境界で切り詰める |
| `drain()` | `WtResult<()>` | WT_DRAIN_SESSION を送信 |
| `send_max_data(maximum: u64)` | `WtResult<()>` | WT_MAX_DATA を送信 |
| `send_max_stream_data(WtStreamId, maximum: u64)` | `WtResult<()>` | WT_MAX_STREAM_DATA を送信 |
| `send_max_streams(maximum: u64, bidirectional: bool)` | `WtResult<()>` | WT_MAX_STREAMS を送信 |
| `flow_control()` / `flow_control_mut()` | `&WtFlowControl` / `&mut WtFlowControl` | フロー制御状態 |
| `stream(WtStreamId)` | `Option<&WtStream>` | ストリーム参照取得 |
| `grow_recv_window(increment: u64)` | `WtResult<()>` | 接続受信ウィンドウを拡張 |
| `grow_stream_recv_window(WtStreamId, increment: u64)` | `WtResult<()>` | ストリーム受信ウィンドウを拡張 |
| `grow_max_streams(increment: u64, bidirectional: bool)` | `WtResult<()>` | 最大ストリーム数を拡張 |
| `config()` | `&WtConfig` | 設定参照 |

### WtConfig / WtInit

`WtConfig` は初期フロー制御値や上限を保持する。

| メソッド | 説明 |
|---------|------|
| `apply_init(&WtInit)` | `WebTransport-Init` ヘッダー値をローカル設定に反映 |
| `apply_init_as_peer(&WtInit)` | ピア側設定として `WebTransport-Init` を反映 |
| `overlay_settings(&Settings)` | HTTP/2 SETTINGS の `SETTINGS_WT_INITIAL_MAX_*` を上書き適用 (`accept()` で自動呼出) |

### WtEvent / WtSessionState

| `WtEvent` バリアント | 説明 |
|--------------------|------|
| `StreamOpened { stream_id, bidirectional }` | ストリームが開かれた |
| `StreamData { stream_id, data, fin }` | ストリームに WT_STREAM Capsule が到着 |
| `StreamReset { stream_id, error_code }` | WT_RESET_STREAM を受信 |
| `StopSending { stream_id, error_code }` | WT_STOP_SENDING を受信 |
| `DatagramReceived { data }` | DATAGRAM を受信 |
| `SessionDraining` | WT_DRAIN_SESSION を受信 |
| `SessionClosed { error_code, reason }` | WT_CLOSE_SESSION を受信 |

フロー制御 Capsule (`WT_MAX_DATA` / `WT_MAX_STREAM_DATA` / `WT_MAX_STREAMS` / `*_BLOCKED`) の受信は内部状態を更新するが、`WtEvent` としては公開しない。

| `WtSessionState` | 説明 |
|----------------|------|
| `Initial` / `Active` / `Draining` / `Closed` | セッション状態遷移 |

### サブプロトコル / Exporter

| 型 / 関数 | 説明 |
|----------|------|
| `WtAvailableProtocols` | `WT-Available-Protocols` ヘッダーのパース結果 (`parse(&[u8]) -> Result<Self, WtError>`、RFC 8941 List of String) |
| `serialize_wt_protocol(&[u8])` | `WT-Protocol` 値を RFC 8941 sf-string としてシリアライズ |
| `serialize_exporter_context(session_id, app_label, app_context)` | TLS Keying Material Exporter 用コンテキストをシリアライズ (draft-15 Section 5.3) |

### Capsule (draft-ietf-webtrans-http2-15 Section 6)

| Capsule | Type コード |
|---------|------------|
| `WT_RESET_STREAM` | `0x190B4D39` |
| `WT_STOP_SENDING` | `0x190B4D3A` |
| `WT_STREAM` (FIN=0, 非終端) | `0x190B4D3C` |
| `WT_STREAM` (FIN=1, 終端) | `0x190B4D3B` |
| `WT_MAX_DATA` | `0x190B4D3D` |
| `WT_MAX_STREAM_DATA` | `0x190B4D3E` |
| `WT_MAX_STREAMS_BIDI` | `0x190B4D3F` |
| `WT_MAX_STREAMS_UNI` | `0x190B4D40` |
| `WT_DATA_BLOCKED` | `0x190B4D41` |
| `WT_STREAM_DATA_BLOCKED` | `0x190B4D42` |
| `WT_STREAMS_BLOCKED_BIDI` | `0x190B4D43` |
| `WT_STREAMS_BLOCKED_UNI` | `0x190B4D44` |
| `WT_CLOSE_SESSION` | `0x2843` |
| `WT_DRAIN_SESSION` | `0x78AE` |

draft-15 では WT_STREAM の LSB が FIN bit。`0x190B4D3C` (LSB=0) が非終端、`0x190B4D3B` (LSB=1) が終端。

### WebTransport SETTINGS

| パラメータ | Identifier |
|----------|-----------|
| `SETTINGS_WT_ENABLED` | `0x2b60` |
| `SETTINGS_WT_INITIAL_MAX_DATA` | `0x2b61` |
| `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI` | `0x2b62` |
| `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL` | `0x2b63` |
| `SETTINGS_WT_INITIAL_MAX_STREAMS_UNI` | `0x2b64` |
| `SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI` | `0x2b65` |
| `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE` | `0x2b66` |

## コード例

### クライアント (Sans I/O)

```rust
use shiguredo_http2::{Connection, Event, HeaderField, Limits};

let limits = Limits::builder()
    .max_concurrent_streams(Some(100))
    .build()?;
let mut conn = Connection::client(limits);

// 接続を開始 (プリフェイスと SETTINGS をキューに積む)
conn.initiate()?;

// 送信バイトを取り出してネットワークに書き込む
while let Some(output) = conn.poll_output() {
    // stream.write_all(&output)?;
}

// リクエストを送信
let headers = vec![
    HeaderField::new(":method", "GET")?,
    HeaderField::new(":scheme", "https")?,
    HeaderField::new(":path", "/")?,
    HeaderField::new(":authority", "example.com")?,
];
let stream_id = conn.start_stream(headers, true)?;

// 受信バイトを投入してイベントを処理
// conn.feed(&received_data)?;
// conn.process()?;
// while let Some(event) = conn.poll_event() {
//     match event {
//         Event::HeadersReceived { stream_id, headers, end_stream, .. } => { /* ... */ }
//         Event::DataReceived { stream_id, data, end_stream } => { /* ... */ }
//         _ => {}
//     }
// }
```

### サーバー (Sans I/O)

```rust
use shiguredo_http2::{Connection, Event, HeaderField, Limits};

let mut conn = Connection::server(Limits::default());

// 接続を開始 (SETTINGS を送信)
conn.initiate()?;

// 受信バイトを投入して処理
// conn.feed(&received_data)?;
// conn.process()?;

while let Some(event) = conn.poll_event() {
    match event {
        Event::HeadersReceived { stream_id, headers, end_stream, .. } => {
            let response_headers = vec![
                HeaderField::new(":status", "200")?,
                HeaderField::new("content-type", "text/plain")?,
            ];
            conn.send_response(stream_id, response_headers, false)?;
            conn.send_data(stream_id, b"Hello, HTTP/2!".to_vec(), true)?;
        }
        _ => {}
    }
}
```

### HPACK エンコード/デコード

```rust
use shiguredo_http2::{HpackDecoder, HpackEncoder, HeaderField};

let mut encoder = HpackEncoder::new(4096);
let headers = vec![
    HeaderField::new(":status", "200")?,
    HeaderField::new("content-type", "text/plain")?,
];
let mut encoded = Vec::new();
encoder.encode(&mut encoded, &headers);

let mut decoder = HpackDecoder::new(4096);
let decoded = decoder.decode(&encoded)?;
```

### WebTransport over HTTP/2 (Sans I/O)

```rust
use shiguredo_http2::webtransport::{WtConfig, WtSession};

let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
session.initiate()?;

let stream_id = session.open_bidi_stream()?;
session.send_stream_data(stream_id, b"Hello", false)?;
session.send_datagram(b"Datagram")?;

// session.feed(&received_data)?;
// session.process()?;
// while let Some(event) = session.poll_event() { /* ... */ }
```

## Tokio 統合 (tokio-http2 / tokio-nghttp2)

Sans I/O 本体を Tokio + Rustls (aws-lc-rs) で非同期化する 2 系統のクレートを提供する。

| クレート | 内部 | 提供型 |
|---------|------|--------|
| `tokio-http2` | shiguredo_http2 (純 Rust) | `Client`, `Server`, `ServerConnection`, `Connection<S>`, `TlsClientConfig`, `TlsServerConfig`, `webtransport::*` |
| `tokio-nghttp2` | shiguredo_nghttp2 (nghttp2 C ライブラリ) | `Client`, `Server`, `ServerConnection`, `Connection<S>`, `TlsClientConfig`, `TlsServerConfig` |

両者とも ALPN は `h2`、TLS は Rustls (aws-lc-rs バックエンド)。Extended CONNECT (RFC 8441) と WebTransport は `tokio-http2` のみ対応する。

### TLS 設定

```rust
use tokio_http2::{TlsClientConfig, TlsServerConfig};

// クライアント
let tls = TlsClientConfig::with_platform_verifier()?;  // OS の証明書ストア
let tls = TlsClientConfig::with_custom_ca(ca_pem)?;    // カスタム CA
let tls = TlsClientConfig::insecure()?;                // テスト用 (検証なし)

// サーバー
let tls = TlsServerConfig::new(cert_pem, key_pem)?;
let tls = TlsServerConfig::from_der(certs, key)?;
```

### tokio-http2 クライアント

```rust
use tokio_http2::{Client, Event, HeaderField, Limits, TlsClientConfig};

let mut client = Client::connect(addr, "example.com", TlsClientConfig::with_platform_verifier()?, Limits::default()).await?;
let stream_id = client.send_request(headers, true).await?;
match client.next_event().await? {
    Event::HeadersReceived { .. } => { /* ... */ }
    Event::DataReceived { .. } => { /* ... */ }
    _ => {}
}
```

### tokio-http2 WebTransport サーバー

`ServerConnection::next_event()` で Extended CONNECT (`:method=CONNECT` かつ `:protocol=webtransport`) を検出したら `WtServerRequest::from_connection()` で要求を組み立てて `accept(WtConfig, allowed_origin, selected_protocol)` する。サーバー側 `Limits` には `enable_connect_protocol(true)` と `wt_enabled(true)` が必要。

```rust
use shiguredo_http2::webtransport::WtConfig;
use tokio_http2::webtransport::{WEBTRANSPORT_PROTOCOL, WtServerRequest};

let request = WtServerRequest::from_connection(conn, stream_id, headers);
let mut session = request
    .accept(WtConfig::default(), Some(b"https://example.com"), None)
    .await?;
while let Some(mut bidi) = session.accept_bidi().await {
    let data = bidi.recv().await?;
    bidi.send(b"hello".to_vec(), true).await?;
}
```

WebTransport 関連型: `WtServerRequest`, `WtServerSession`, `WtSessionHandle`, `WtSessionParts`, `WtBidiStream`, `WtUniRecvStream`, `WtUniSendStream`, 定数 `WEBTRANSPORT_PROTOCOL = b"webtransport"`。

`accept(config, allowed_origin, selected_protocol)` の挙動:

- TLS 1.3 必須 (draft-ietf-webtrans-http2-15 Section 7。rustls 0.23 が TLS 1.2 + extended master secret の状態を公開しないため仕様より厳しい)
- `:scheme` が `https` でない場合は `RST_STREAM(PROTOCOL_ERROR)` で拒否 (Section 3.2)
- `allowed_origin: Some(_)` のとき、Origin ヘッダーが存在する場合のみ検証する。欠落時は検証スキップ。不一致は 403
- `selected_protocol: Some(_)` のとき、リクエストの `WT-Available-Protocols` に含まれることを検証し、レスポンスに `wt-protocol` を付与 (Section 3.3)
- `WebTransport-Init` のパース失敗時は `:status=400` で拒否
- 自広告 / ピア SETTINGS を `WtConfig::overlay_settings` で自動適用 (Section 4.3.1)

`WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material` で TLS Keying Material Exporter を利用できる (Section 5.3)。

### nghttp2 系 (tokio-nghttp2 / shiguredo_nghttp2)

nghttp2 C ライブラリを使う系統。`shiguredo_nghttp2::Session` を中心に以下の API を提供する。

| 型 | 説明 |
|----|------|
| `Session` | nghttp2 セッション。`client()` / `server()` / `client_with_options(&SessionOptions)` / `server_with_options(&SessionOptions)` |
| `SessionOptions` | DoS 対策・フロー制御挙動のオプション Builder (`no_auto_window_update`, `peer_max_concurrent_streams`, `no_auto_ping_ack`, `max_send_header_block_length`, `max_deflate_dynamic_table_size`, `max_outbound_ack`, `max_settings`, `stream_reset_rate_limit`, `max_continuations`, `glitch_rate_limit`) |
| `Http2Event` | nghttp2 由来のイベント (`HeadersReceived`, `DataReceived`, `StreamClosed`, `GoawayReceived`, `PingReceived`, `SettingsReceived`, `WindowUpdateReceived`, `FrameSent`, `FrameNotSent`, `InvalidFrameReceived`, `InvalidHeaderReceived`) |
| `Header` | nghttp2 用ヘッダー (擬似ヘッダーのヘルパー `method()` / `scheme()` / `authority()` / `path()` / `status()` / `sensitive()`) |
| `ErrorCode` / `FrameType` / `SettingsId` / `StreamId` | HTTP/2 各種定数 |

`Session` の主要メソッド: `recv()`, `send()`, `poll_event()`, `want_read()` / `want_write()`, `submit_settings()`, `submit_request(headers, data: Option<&[u8]>, end_stream)`, `submit_response()`, `submit_data()`, `submit_data_for_trailer()`, `submit_trailer()`, `submit_headers()`, `submit_rst_stream()`, `submit_goaway()`, `submit_ping()`, `submit_window_update()`, `submit_shutdown_notice()`, `terminate_session()`, `get_*_settings()`, `get_*_window_size()`, `set_local_window_size()`, `consume()` / `consume_connection()` / `consume_stream()`, `last_error_message()`。

`tokio-nghttp2` の `Client` / `Server` / `ServerConnection` はこれを Tokio で非同期化する薄いラッパー。

## エラー型

### `Error` (HTTP/2 接続エラー)

`Error` は `ErrorKind` と `reason` / `location` / `backtrace` を内包する。フィールドは private のため、`kind()` / `reason()` / `location()` / `backtrace()` の getter 経由でのみ読み取れる。`Error::connection_error(ErrorCode, reason)` / `Error::stream_error(ErrorCode, reason)` / `Error::hpack_error(reason)` / `Error::protocol_error(reason)` / `Error::frame_size_error(reason)` で作成する。`is_connection_error()` / `is_stream_error()` / `error_code()` で分類できる。

| `ErrorKind` バリアント | 説明 |
|--------------------|------|
| `ConnectionError(ErrorCode)` | 接続レベルエラー (GOAWAY 送信対象) |
| `StreamError(ErrorCode)` | ストリームレベルエラー (RST_STREAM 送信対象) |
| `HpackError` | HPACK デコードエラー (RFC 7541)。詳細は `Error::reason()` で読み取る |

`protocol_error` / `frame_size_error` はいずれも `ConnectionError` を構築するヘルパー。

### `ErrorCode` (HTTP/2 エラーコード)

RFC 9113 §7 の HTTP/2 エラーコードに加え、draft-ietf-webtrans-http2-15 Section 3.4 / Section 11.3 の WebTransport エラーコードを含む。`as_u32()` / `from_u32(u32)` で変換可能。

| バリアント | コード |
|----------|-------|
| `NoError` | `0x00` |
| `ProtocolError` | `0x01` |
| `InternalError` | `0x02` |
| `FlowControlError` | `0x03` |
| `SettingsTimeout` | `0x04` |
| `StreamClosed` | `0x05` |
| `FrameSizeError` | `0x06` |
| `RefusedStream` | `0x07` |
| `Cancel` | `0x08` |
| `CompressionError` | `0x09` |
| `ConnectError` | `0x0a` |
| `EnhanceYourCalm` | `0x0b` |
| `InadequateSecurity` | `0x0c` |
| `Http11Required` | `0x0d` |
| `WtError` | `0x100` (`WT_ERROR`) |
| `WtStreamStateError` | `0x101` (`WT_STREAM_STATE_ERROR`) |
| `WtFlowControlError` | `0x102` (`WT_FLOW_CONTROL_ERROR`) |

注: `ErrorCode::WtError` は HTTP/2 GOAWAY/RST_STREAM 用のエラーコードであり、`webtransport::WtError` 構造体とは別物。

### `tokio_http2::Error`

| バリアント | 説明 |
|----------|------|
| `Io(io::Error)` | I/O エラー |
| `Protocol(shiguredo_http2::Error)` | HTTP/2 プロトコルエラー |
| `Tls(...)` | TLS エラー |
| `WebTransport(WtError)` | WebTransport セッション / Capsule 処理エラー |
| `ConnectionClosed` | 接続クローズ |
| `InvalidArgument(String)` | 無効な引数 |

### その他

- `DecodeError`: フレームデコード時のバイト列エラー
- `FrameError`: フレーム構造エラー
- `SettingError`: SETTINGS 値検証エラー
- `LimitsError`: `Limits` 構築時のエラー (`WebtransportRequiresConnectProtocol` / `WebtransportRequiresWtEnabled`)
- `StreamIdError`: ストリーム ID 構築エラー
- `HeaderFieldError`: HPACK ヘッダー構築エラー (CRLF/NUL 拒否)
- `WtError` / `WtErrorKind`: WebTransport 層のエラー (`Incomplete`, `BufferTooShort`, `InvalidInput`, `CapsuleDecode`, `InvalidStreamId`, `StreamStateError`, `FlowControlError`, `SessionStateError`)

## 対応仕様

| RFC / draft | 名称 | 対応機能 |
|------------|------|---------|
| RFC 7541 | HPACK: Header Compression for HTTP/2 | 静的/動的テーブル、Huffman、整数符号化 |
| RFC 8441 | Bootstrapping WebSockets with HTTP/2 | Extended CONNECT (`:protocol` 擬似ヘッダー) |
| RFC 9113 | HTTP/2 | フレーム、フロー制御、ストリーム状態遷移、CONNECT、エラー処理 |
| RFC 9218 | Extensible Prioritization Scheme for HTTP | PRIORITY_UPDATE フレーム、`urgency` / `incremental` |
| RFC 9297 | HTTP Datagrams and the Capsule Protocol | Capsule, DATAGRAM, PADDING |
| draft-ietf-webtrans-http2-15 | WebTransport over HTTP/2 | WT_* Capsule, WebTransport SETTINGS, Extended CONNECT (`webtransport`), TLS 1.3 要件, Origin 検証 |

## 既知の未対応 / 制限

- **サーバープッシュ (PUSH_PROMISE)**: 主要ブラウザ (Chrome / Firefox / Safari) が削除済みのため未対応。受信した場合は `PROTOCOL_ERROR` で GOAWAY する (`src/connection/mod.rs` の `Frame::PushPromise` 分岐)。
- **WebSocket over HTTP/2**: Extended CONNECT (RFC 8441) は対応しているが、WebSocket フレーム層は未実装。
- **PRIORITY フレーム**: RFC 9113 で非推奨。受信は処理する (優先度情報は無視) が、送信はしない。
- **HEADERS の優先度フィールド**: RFC 9113 で非推奨。
- **tokio-nghttp2 と Extended CONNECT / WebTransport**: tokio-nghttp2 側は Extended CONNECT と WebTransport を提供しない (tokio-http2 のみ)。
- **tokio-http2 の WebTransport accept は TLS 1.3 必須**: rustls 0.23 が TLS 1.2 + extended master secret のネゴシエーション状態を外部公開していないため、安全側に倒して TLS 1.3 のみを許可する (draft-ietf-webtrans-http2-15 Section 7 の要件より厳しい)。
- **WebTransport の二重ゲート**: `:protocol=webtransport` の CONNECT 開始にはピアの `SETTINGS_ENABLE_CONNECT_PROTOCOL=1` と `SETTINGS_WT_ENABLED=1` の両方が必要。
