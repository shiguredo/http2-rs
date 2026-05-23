# StreamId を NewType 化し奇偶ルールを型で表現する

Created: 2026-05-23
Model: Opus 4.7

## 概要

現状 `StreamId` は単純な `pub type StreamId = u32;` のエイリアス。RFC 9113 §5.1.1 で定義される
以下のルールが型で表現されておらず、各所で `if stream_id == 0`, `if stream_id % 2 == 0` のような
条件分岐が散在する。

- stream_id = 0 は接続レベル制御専用
- クライアントが開始するストリーム ID は奇数 (1, 3, 5, ...)
- サーバーが開始するストリーム ID は偶数 (2, 4, 6, ...)
- ID は単調増加 MUST、上限 2^31 - 1

これを `enum` + `NonZeroU32` で表現し、構築時およびコンパイル時 (リテラル) に違反を検出可能にする。

## 背景

現状の問題:

- `src/frame/mod.rs`: `pub type StreamId = u32;`
- `src/connection/mod.rs` 各所で `if stream_id == 0` / `validate_stream_id_parity` が散在
- `DataFrame::new(stream_id: 0, ...)` のような不正値を構築可能
- 「接続制御用 (0)」と「ストリーム用 (>0)」を関数シグネチャで区別できないため、
  呼び出し側が常にチェックを書く必要がある

## 根拠

- RFC 9113 §5.1.1: "Streams are identified with an unsigned 31-bit integer. ...
  Streams initiated by a client MUST use odd-numbered stream identifiers; those
  initiated by the server MUST use even-numbered stream identifiers. A stream
  identifier of zero (0x00) is used for connection control messages"
- RFC 9113 §5.1.1: "Stream identifiers cannot be reused"
- 同 §6.1 DATA / §6.2 HEADERS / §6.4 RST_STREAM / §6.6 PUSH_PROMISE / §6.10 CONTINUATION
  は stream_id = 0 で受信すると PROTOCOL_ERROR (接続エラー)
- §6.5 SETTINGS / §6.7 PING / §6.8 GOAWAY は stream_id = 0 以外で受信すると PROTOCOL_ERROR
- 型でこの区別を表現すれば、フレーム種別ごとに適切な ID 種別だけを受け取れる

## 設計

### 型定義

```rust
use core::num::NonZeroU32;

/// HTTP/2 ストリーム識別子 (RFC 9113 §5.1.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StreamId {
    /// 接続レベル制御用 (0)
    Connection,
    /// クライアント開始ストリーム (奇数)
    Client(ClientStreamId),
    /// サーバー開始ストリーム (偶数)
    Server(ServerStreamId),
}

/// クライアント開始ストリーム ID (奇数のみ)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientStreamId(NonZeroU32);

/// サーバー開始ストリーム ID (偶数のみ)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerStreamId(NonZeroU32);

impl ClientStreamId {
    pub fn new(id: u32) -> Result<Self, StreamIdError>;
    pub const fn from_static(id: u32) -> Self;  // 不正値でコンパイル時 panic
    pub fn get(self) -> NonZeroU32;
}

// ServerStreamId も同様

impl StreamId {
    pub fn from_wire(id: u32) -> Self;  // wire 上の任意 u32 を分類のみ実施
    pub const fn as_u32(self) -> u32;
}
```

### 構築時検査内容

- `ClientStreamId::new(id)`:
  - id = 0 → `StreamIdError::Reserved` (接続制御用)
  - id 偶数 → `StreamIdError::ParityMismatch { expected: Odd }`
  - id >= 2^31 → `StreamIdError::OutOfRange`
- `ServerStreamId::new(id)`:
  - id = 0 → `StreamIdError::Reserved`
  - id 奇数 → `StreamIdError::ParityMismatch { expected: Even }`
  - id >= 2^31 → `StreamIdError::OutOfRange`
- `from_static` (const fn) では panic、つまりコンパイルエラーで通知

### フレーム API への適用

```rust
// 変更前
impl DataFrame {
    pub fn new(stream_id: u32, data: Vec<u8>) -> Self;
}

// 変更後 (DATA は接続レベル 0 不可)
impl DataFrame {
    pub fn new(stream_id: NonZeroStreamId, data: Vec<u8>) -> Self;
}

// SettingsFrame / PingFrame / GoawayFrame は逆に Connection のみ受け取る
impl SettingsFrame {
    pub fn new() -> Self;  // 内部で StreamId::Connection を保持
    // stream_id 引数を受け取らない
}
```

`NonZeroStreamId` (Client or Server、Connection 以外) という補助型を `StreamId` から
派生させ、stream_id = 0 を構造的に持てないフレーム構築点で使う。

## 影響範囲

- `src/frame/mod.rs`: `StreamId` 型定義変更、全フレーム構造体の `stream_id` フィールド型変更
- `src/frame/decoder.rs`: wire 上の u32 → `StreamId` 変換と、フレーム種別ごとの ID 種別検査
- `src/frame/encoder.rs`: `as_u32` で書き出し
- `src/connection/mod.rs`: 1974 行のうち `stream_id == 0` チェック群が削減
- `src/stream/`: `Stream` の ID 型変更
- `src/validation.rs`: `validate_stream_id_parity` を `ClientStreamId::new` / `ServerStreamId::new` に統合
- `src/flow_control.rs`: 直接影響なし
- `tests/`, `pbt/`, `fuzz/`, `examples/`: API 追従

## CHANGES.md エントリ

```
- [CHANGE] `StreamId` を `u32` エイリアスから `enum StreamId { Connection, Client(_), Server(_) }`
  に変更し、奇偶ルールと接続制御 ID を型で表現する
- [ADD] `ClientStreamId::from_static` / `ServerStreamId::from_static` を `const fn` で
  追加し、不正なリテラル ID をコンパイル時に検出可能にする
```

## 受け入れ条件

- `StreamId` が enum で定義され、`Connection` / `Client(ClientStreamId)` / `Server(ServerStreamId)` に分かれている
- `ClientStreamId::new` / `ServerStreamId::new` が `Result<Self, StreamIdError>` を返す
- `*::from_static` が `const fn` で実装され、不正リテラルでコンパイルエラーになる
- DATA / HEADERS / RST_STREAM / WINDOW_UPDATE (stream-level) / PUSH_PROMISE / CONTINUATION
  の構築点が `NonZeroStreamId` を受け取る
- SETTINGS / PING / GOAWAY / WINDOW_UPDATE (connection-level) は stream_id 引数を取らないか、
  `StreamId::Connection` のみを受け付ける
- `validate_stream_id_parity` が削除または `ClientStreamId::new` / `ServerStreamId::new` に統合されている
- 既存の全テスト・PBT・fuzz が通る

## 関連

- [[0024-change-header-field-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]]
- [[0029-change-split-error-types]]
- 既存 issue 0023 の §2 (`validate_stream_id_parity` の無意味な role match) は本 issue で解消
