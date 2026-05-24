# 各 Frame 型を構築時検査型に変更する

Created: 2026-05-23
Model: Opus 4.7

## 概要

各 HTTP/2 フレーム型 (`DataFrame`, `HeadersFrame`, `RstStreamFrame`, `WindowUpdateFrame`,
`PingFrame`, `GoawayFrame`, `ContinuationFrame`, `PriorityUpdateFrame`) のコンストラクタを
構築時検査つきに作り変える。

`*::new` を `Result` 化し、リテラル定数向けに `*::from_static` (`const fn`) も提供する。
構築時に検査することで、不正な値を持ったフレームを `Connection::send_frame` まで持ち回せなくする。

## 背景

現状の問題:

- `DataFrame::new(0, vec![])` のように stream_id = 0 の DATA を構築可能 (PROTOCOL_ERROR)
- `WindowUpdateFrame::new(stream_id, 0)` のように increment = 0 を構築可能 (PROTOCOL_ERROR)
- `RstStreamFrame::new(0, ErrorCode::Cancel)` のように stream_id = 0 の RST_STREAM を構築可能
- `PingFrame` の payload が 8 バイト固定であることを型で表現していない (現状は `[u8; 8]` だが、
  ACK 状態と opaque_data の組み合わせ制約等が散在)
- `GoawayFrame` の last_stream_id が 2^31 を超える値を持てる

## 根拠

RFC 9113 §6 各フレーム定義:

- **DATA (§6.1)**: stream_id = 0 は PROTOCOL_ERROR。padding length が payload を超えると PROTOCOL_ERROR
- **HEADERS (§6.2)**: stream_id = 0 は PROTOCOL_ERROR。padding length が payload を超えると PROTOCOL_ERROR。
  PRIORITY フラグ時の weight は 1-256 (wire 上 0-255 で +1 する)
- **PRIORITY (§6.3)**: 廃止 (RFC 9113 で deprecate)、構築 API は提供しない
- **RST_STREAM (§6.4)**: stream_id = 0 は PROTOCOL_ERROR。payload は 4 バイト固定
- **SETTINGS (§6.5)**: stream_id != 0 は PROTOCOL_ERROR。payload 長が 6 の倍数でないと FRAME_SIZE_ERROR
- **PUSH_PROMISE (§6.6)**: 本ライブラリで送信非対応の場合は構築 API を提供しない
- **PING (§6.7)**: stream_id != 0 は PROTOCOL_ERROR。payload は 8 バイト固定
- **GOAWAY (§6.8)**: stream_id != 0 は PROTOCOL_ERROR。last_stream_id は 2^31 - 1 以下
- **WINDOW_UPDATE (§6.9)**: increment = 0 は PROTOCOL_ERROR (stream-level は STREAM_ERROR)。
  増分は 2^31 - 1 以下
- **CONTINUATION (§6.10)**: stream_id = 0 は PROTOCOL_ERROR
- **PRIORITY_UPDATE (RFC 9218 §7.1)**: stream_id != 0 は PROTOCOL_ERROR

## 設計

### DataFrame

```rust
impl DataFrame {
    pub fn new(stream_id: NonZeroStreamId, data: impl Into<Bytes>) -> Self;
    pub fn with_padding(stream_id: NonZeroStreamId, data: Bytes, padding: u8)
        -> Result<Self, FrameError>;
    pub fn end_stream(mut self) -> Self;
    pub const fn from_static(stream_id: NonZeroStreamId, data: &'static [u8]) -> Self;
}
```

stream_id は `NonZeroStreamId` (issue 0025) を要求するため、構築点で必ず非 0。
padding 長は payload 長以下である必要があり、`with_padding` は `Result` を返す。

### HeadersFrame

```rust
impl HeadersFrame {
    pub fn new(stream_id: NonZeroStreamId, headers: Vec<HeaderField>) -> Self;
    pub fn with_priority(
        stream_id: NonZeroStreamId,
        headers: Vec<HeaderField>,
        priority: Priority,
    ) -> Self;
    pub fn end_stream(mut self) -> Self;
    pub fn end_headers(mut self) -> Self;
}

/// HEADERS の優先度情報 (RFC 9113 で deprecate されたが互換性のため残す)
pub struct Priority {
    pub stream_dependency: NonZeroStreamId,
    pub weight: Weight,  // 1-256 を型で表現
    pub exclusive: bool,
}

pub struct Weight(u16);  // 1..=256

impl Weight {
    pub fn new(weight: u16) -> Result<Self, FrameError>;
    pub const fn from_static(weight: u16) -> Self;
    pub const fn get(self) -> u16;
}
```

`headers: Vec<HeaderField>` は各要素が既に検証済み (issue 0024)。

### RstStreamFrame

```rust
impl RstStreamFrame {
    pub const fn new(stream_id: NonZeroStreamId, error_code: ErrorCode) -> Self;
}
```

stream_id を型で非 0 強制、error_code は既に enum なので追加検査不要。`const fn` 化可能。

### WindowUpdateFrame

```rust
pub struct WindowIncrement(NonZeroU32);  // 1..=2^31-1

impl WindowIncrement {
    pub fn new(increment: u32) -> Result<Self, FrameError>;
    pub const fn from_static(increment: u32) -> Self;
    pub const fn get(self) -> NonZeroU32;
}

impl WindowUpdateFrame {
    pub const fn for_connection(increment: WindowIncrement) -> Self;
    pub const fn for_stream(stream_id: NonZeroStreamId, increment: WindowIncrement) -> Self;
}
```

increment = 0 を型で構築不能にする。`MAX_WINDOW_SIZE` 超過は `WindowIncrement::new` で弾く。

### PingFrame

```rust
impl PingFrame {
    pub const fn new(opaque_data: [u8; 8]) -> Self;
    pub const fn ack(opaque_data: [u8; 8]) -> Self;
}
```

stream_id は型で 0 固定 (引数を取らない)。

### GoawayFrame

```rust
pub struct LastStreamId(u32);  // 0..=2^31-1, 0 も合法 (どのストリームも処理していない場合)

impl GoawayFrame {
    pub fn new(
        last_stream_id: LastStreamId,
        error_code: ErrorCode,
        debug_data: impl Into<Bytes>,
    ) -> Self;
    pub const fn from_static(
        last_stream_id: LastStreamId,
        error_code: ErrorCode,
        debug_data: &'static [u8],
    ) -> Self;
}
```

### ContinuationFrame

```rust
impl ContinuationFrame {
    pub fn new(stream_id: NonZeroStreamId, header_block_fragment: Vec<u8>) -> Self;
    pub fn end_headers(mut self) -> Self;
}
```

### PriorityUpdateFrame (RFC 9218)

```rust
impl PriorityUpdateFrame {
    pub fn new(
        prioritized_stream_id: NonZeroStreamId,
        priority_field_value: impl Into<Bytes>,
    ) -> Self;
}
```

### PriorityFrame (RFC 9113 §6.3 deprecated)

公開コンストラクタは提供しない (送信非対応)。decoder が受信したフレームを構築するために
`pub(crate)` のコンストラクタを用意する。`Frame::Priority` variant は維持する。

### PushPromise (RFC 9113 §6.6)

本ライブラリでは送信非対応。公開コンストラクタは提供しない。decoder が受信したフレームを
構築するために `pub(crate)` のコンストラクタを用意する。

### HeadersFrame のパディング

現行の `HeadersFrame` は `pad_length: Option<u8>` を持つ (RFC 9113 §6.2 PADDED フラグ)。
新設計でも維持する。`DataFrame` と同様に `with_padding` メソッドを提供する:

```rust
impl HeadersFrame {
    pub fn with_padding(
        stream_id: NonZeroStreamId,
        headers: Vec<HeaderField>,
        padding: u8,
    ) -> Result<Self, FrameError>;
}
```

### SettingsFrame / PingFrame / GoawayFrame

これらのフレームは stream_id が 0 固定のため、コンストラクタから stream_id 引数を除去する。
`SettingsFrame` のフィールド private 化は issue 0026 で扱う。

## 影響範囲

- `src/frame/mod.rs`: 各フレーム型の API 変更、補助型 (`Weight`, `WindowIncrement`,
  `LastStreamId`) 追加。`NonZeroStreamId` は issue 0025 で定義済み
- `src/frame/decoder.rs`: 補助型経由で構築、不正値は接続/ストリームエラーに変換
- `src/frame/encoder.rs`: API 追従
- `src/connection/mod.rs`: 送信側の事後検査ロジックを削減
- `tests/`, `pbt/`, `fuzz/`, `examples/`: API 追従

## CHANGES.md エントリ

```
- [ADD] `Weight` / `WindowIncrement` / `LastStreamId` の補助型を追加し、
  RFC 9113 の値範囲制約を型で表現する
  - @担当者
- [ADD] フレーム構築の `const fn from_static` 系を追加し、不正リテラルをコンパイル時に
  検出可能にする
  - @担当者
- [CHANGE] `DataFrame` / `HeadersFrame` / `RstStreamFrame` / `WindowUpdateFrame` /
  `PingFrame` / `GoawayFrame` / `ContinuationFrame` / `PriorityUpdateFrame` の構築 API を
  構築時検査つきに変更する
  - @担当者
```

## 受け入れ条件

- DATA / HEADERS / RST_STREAM / WINDOW_UPDATE (stream) / CONTINUATION / PRIORITY_UPDATE が
  `NonZeroStreamId` を要求する
- SETTINGS / PING / GOAWAY が stream_id 引数を取らない (内部で Connection 固定)
- `WindowIncrement` / `Weight` / `LastStreamId` が範囲制約を型で表現している
- 各補助型に `from_static` (`const fn`) が実装され、不正リテラルでコンパイルエラーになる
- PriorityFrame / PushPromise は公開コンストラクタを提供せず、`pub(crate)` のみ
- decoder で構築する経路は不正値を接続エラー or ストリームエラーに変換している
- `Connection::send_*` 経路の事後値検査が削減されている
- 既存の全テスト・PBT・fuzz が通る

## 依存

- [[0025-change-stream-id-newtype]] (`NonZeroStreamId` を提供)
- [[0024-change-header-field-construct-time-validation]] (`HeaderField` 構築時検査)
- [[0029-change-split-error-types]] (基盤の `DecodeError` / `SettingError` を提供)
- [[0013-refactor-bytes-payloads]] (Bytes 化と統合)

## 0029 から引き継ぐ作業

`SendError` の定義および `src/connection/mod.rs` のクライアント側送信前検査
(`Error::protocol_error` / `Error::stream_error` の一部) を `SendError` に置き換える
作業は、本 issue 0027 で `FrameError` と同時に設計・実装する。

`SendError` は当初 issue 0029 で導入する設計だったが、フレーム送信 API の
コンテキストに密接しているため、`FrameError` と同時に決定する方が境界が明確になる。

variant 候補 (実コードに対応する):

- `ServerCannotInitiateStream` (server push 不許可、`connection/mod.rs:401`)
- `MaxConcurrentStreamsExceeded { current: usize, limit: u32 }` (`connection/mod.rs:422`)
- `ExtendedConnectNotEnabled` (`connection/mod.rs:435`、RFC 8441 §3)
- `HeaderListTooLarge { actual: usize, limit: u32 }`
  (`connection/mod.rs:444 / 787 / 869`、RFC 9113 §10.5.1)

最終的な variant は本 issue 着手時に再評価する。
