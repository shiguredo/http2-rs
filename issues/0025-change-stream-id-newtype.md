# StreamId を NewType 化し奇偶ルールを型で表現する

Created: 2026-05-23
Model: Opus 4.7

## 概要

現状 `StreamId` は `pub type StreamId = u32;` の単純な型エイリアス。RFC 9113 §5.1.1 の
ルールが型で表現されておらず、`stream_id == 0` / `stream_id % 2 == 0` のような条件分岐が
decoder (`src/frame/decoder.rs` に 14 箇所) と connection (`src/connection/mod.rs` に 3 箇所以上)
に散在する。

これを `enum StreamId` + `NonZeroU32` ベースの newtype で表現し、接続制御用 (0) /
クライアント開始 (奇数) / サーバー開始 (偶数) の区別を型レベルで強制する。
リテラル定数向けの `const fn` 構築 API も提供する。

本 issue は `StreamId` 型ファミリーの定義と構築時検査に専念する。各フレーム型への
適用 (コンストラクタ引数型の変更) は issue 0027 で扱う。

## 背景

現状の問題:

- `src/frame/mod.rs`: `pub type StreamId = u32;` (19 行目)
- `DataFrame::new(stream_id: 0, ...)` のような不正値を構築可能
- decoder で全フレーム種別ごとに `stream_id == CONNECTION_STREAM_ID` チェックが散在
- `validate_stream_id_parity` (`src/connection/mod.rs:1763`) で role ベースの偶奇チェックが
  冗長な match 分岐になっている (issue 0023 §2 で指摘済み)

## 根拠

- RFC 9113 §5.1.1: "Streams are identified by an unsigned 31-bit integer. ...
  Streams initiated by a client MUST use odd-numbered stream identifiers; those
  initiated by the server MUST use even-numbered stream identifiers. A stream
  identifier of zero (0x00) is used for connection control messages"
- RFC 9113 §5.1.1: "Stream identifiers cannot be reused"
- RFC 9113 §5.1.1: 新ストリームの ID は過去のストリームより数値的に大きくなければならない (MUST)。
  ただしスキップ (飛び番) は許容される。この単調増加ルールは接続レベルのランタイム検査であり、
  型では表現しない
- RFC 9113 §6.1 DATA / §6.2 HEADERS / §6.3 PRIORITY / §6.4 RST_STREAM /
  §6.6 PUSH_PROMISE / §6.10 CONTINUATION は stream_id = 0 で受信すると PROTOCOL_ERROR
- RFC 9113 §6.5 SETTINGS / §6.7 PING / §6.8 GOAWAY は stream_id != 0 で受信すると PROTOCOL_ERROR

## 設計

### 型定義

```rust
use core::num::NonZeroU32;

/// HTTP/2 ストリーム識別子 (RFC 9113 §5.1.1)
///
/// PartialOrd / Ord は derive しない。variant をまたいだ順序比較は
/// 意味的に不適切 (Client(5) < Server(2) のような直感に反する結果を避ける)。
/// 順序比較が必要な箇所では as_u32() で u32 に変換してから比較する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamId {
    /// 接続レベル制御用 (0)
    Connection,
    /// クライアント開始ストリーム (奇数)
    Client(ClientStreamId),
    /// サーバー開始ストリーム (偶数)
    Server(ServerStreamId),
}

/// 非ゼロストリーム ID (Client または Server)
/// stream_id = 0 を構造的に持てないフレーム構築点で使う (issue 0027 で適用)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NonZeroStreamId {
    Client(ClientStreamId),
    Server(ServerStreamId),
}

/// クライアント開始ストリーム ID (奇数のみ、1..=2^31-1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientStreamId(NonZeroU32);

/// サーバー開始ストリーム ID (偶数のみ、2..=2^31-2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerStreamId(NonZeroU32);
```

### 構築 API

```rust
impl ClientStreamId {
    pub fn new(id: u32) -> Result<Self, StreamIdError>;
    pub const fn from_static(id: u32) -> Self;  // 不正値でコンパイル時 panic
    pub const fn get(self) -> NonZeroU32;
    pub const fn as_u32(self) -> u32;
}

// ServerStreamId も同様

impl NonZeroStreamId {
    pub fn new(id: u32) -> Result<Self, StreamIdError>;  // 奇偶で自動分類
    pub const fn from_static(id: u32) -> Self;
    pub const fn as_u32(self) -> u32;
    pub fn client(self) -> Option<ClientStreamId>;
    pub fn server(self) -> Option<ServerStreamId>;
}

impl StreamId {
    /// wire 上の u32 を分類する (検査ではなく分類のみ)
    /// 呼び出し元が 31-bit マスク済み (id < 2^31) であることを前提とする。
    /// decoder の decode_header が上位 1 ビットをマスクするため、この前提は常に成立する。
    /// 31-bit マスク前の値を渡した場合は debug_assert で panic する。
    pub fn from_wire(id: u32) -> Self;
    pub const fn as_u32(self) -> u32;
    /// Connection variant の場合は None を返す
    pub fn non_zero(self) -> Option<NonZeroStreamId>;
}

impl From<ClientStreamId> for StreamId { ... }
impl From<ServerStreamId> for StreamId { ... }
impl From<NonZeroStreamId> for StreamId { ... }
impl From<ClientStreamId> for NonZeroStreamId { ... }
impl From<ServerStreamId> for NonZeroStreamId { ... }
```

### 構築時検査内容

- `ClientStreamId::new(id)`:
  - id = 0 → `StreamIdError::Reserved`
  - id 偶数 → `StreamIdError::ParityMismatch { expected: Parity::Odd, got: id }`
  - id >= 2^31 → `StreamIdError::OutOfRange { value: id }`
- `ServerStreamId::new(id)`:
  - id = 0 → `StreamIdError::Reserved`
  - id 奇数 → `StreamIdError::ParityMismatch { expected: Parity::Even, got: id }`
  - id >= 2^31 → `StreamIdError::OutOfRange { value: id }`
- `NonZeroStreamId::new(id)`:
  - id = 0 → `StreamIdError::Reserved`
  - id >= 2^31 → `StreamIdError::OutOfRange { value: id }`
  - 奇数 → `Ok(NonZeroStreamId::Client(...))`、偶数 → `Ok(NonZeroStreamId::Server(...))`
- `from_static` (const fn) では panic メッセージで通知

### `FrameHeader` との関係

wire からパースされた `FrameHeader` の `stream_id` は引き続き `u32` のまま保持する。
decoder が各フレーム種別を組み立てる際に `StreamId::from_wire(header.stream_id)` で
変換し、フレーム種別ごとの制約 (0 禁止 / 0 必須) を検査する。

```rust
pub struct FrameHeader {
    pub length: u32,
    pub frame_type: FrameType,
    pub flags: Flags,
    pub stream_id: u32,  // wire レベルの raw 値を保持
}
```

### `validate_stream_id_parity` の統合

`validate_stream_id_parity` (`src/connection/mod.rs:1763`) を削除する。

parity チェック自体は `ClientStreamId::new` / `ServerStreamId::new` に統合される。
role ベースの検査 (サーバーはクライアント開始 ID のみ受信、クライアントはサーバー開始 ID のみ受信)
は、`handle_headers` 等で `StreamId` の variant を match することで実現する:

```rust
// 変更前: validate_stream_id_parity(frame.stream_id)?
// 変更後:
match (self.role, &frame.stream_id) {
    (Role::Server, StreamId::Client(_)) => { /* OK: サーバーがクライアント開始を受信 */ }
    (Role::Client, StreamId::Server(_)) => { /* OK: クライアントがサーバー開始を受信 */ }
    (_, StreamId::Connection) => {
        return Err(Error::protocol_error("stream ID 0 on HEADERS"));
    }
    _ => {
        return Err(Error::protocol_error(format!(
            "unexpected stream ID parity: {}", frame.stream_id.as_u32()
        )));
    }
}
```

### `GoawayFrame::last_stream_id` の扱い

`GoawayFrame::last_stream_id` は 0 (どのストリームも処理していない) も合法。
値範囲は 0..=2^31-1 であり、奇偶の制約はない。専用型 `LastStreamId` を導入する
(issue 0027 で定義)。`LastStreamId` は `StreamId` enum とは独立したラッパ型とし、
`StreamId` との直接変換は行わない (`as_u32()` 経由の比較のみ)。

## 影響範囲

- `src/frame/mod.rs`: `StreamId` 型エイリアス → enum 定義に変更。`CONNECTION_STREAM_ID` 定数を
  `StreamId::Connection` に置き換え。`NonZeroStreamId`, `ClientStreamId`, `ServerStreamId` を追加。
  `Frame::stream_id()` の戻り値型を新 `StreamId` に変更
- `src/frame/decoder.rs`: `StreamId::from_wire` 経由で変換。`last_decoded_stream_id` を
  `Option<StreamId>` に変更。各フレーム種別の stream_id 検査を型ベースに書き換え
- `src/frame/encoder.rs`: `as_u32()` で wire エンコード
- `src/connection/mod.rs`:
  - `validate_stream_id_parity` 削除
  - `streams: HashMap<StreamId, Stream>` → `HashMap<NonZeroStreamId, Stream>`
  - `closed_streams: HashSet<StreamId>` → `HashSet<NonZeroStreamId>`
  - `next_stream_id: StreamId` → `NonZeroStreamId` (初期値は role に応じて
    `ClientStreamId(1)` / `ServerStreamId(2)`)。`+= 2` は
    `id.as_u32().checked_add(2)` でオーバーフロー検出し、`NonZeroStreamId::new(...)` で
    範囲検査。上限到達時 (>= 2^31) は `REFUSED_STREAM` 接続エラー
  - `last_recv_stream_id: StreamId` → `Option<NonZeroStreamId>` (初期値 None)。
    大小比較は `as_u32()` 経由: `new_id.as_u32() > last.map_or(0, |id| id.as_u32())`
  - `last_successful_stream_id: StreamId` → `Option<NonZeroStreamId>` (初期値 None、同上)
  - `header_continuation_stream: Option<StreamId>` → `Option<NonZeroStreamId>`
  - `is_idle_stream` / `check_not_idle_stream` の偶数判定を variant match に変更。
    `StreamId::Connection` が渡された場合は panic (`debug_assert!` で不変条件として表明)
  - `stream_id == 0` チェックを variant match に置き換え
  - `send_window_update` の接続レベル / ストリームレベル分岐を型で表現
- `src/stream/mod.rs`: `Stream.id: StreamId` → `Stream.id: NonZeroStreamId`
- `src/event.rs`: 全 `Event` variant の `stream_id: StreamId` 型を適切に変更。
  `WindowUpdateReceived` は接続レベル (0) も含むため `StreamId` のまま。
  `Event::stream_id()` は `Option<NonZeroStreamId>` を返す (破壊的変更:
  接続レベル WINDOW_UPDATE が `Some(0)` → `None` に変わる)
- `src/lib.rs`: `CONNECTION_STREAM_ID` の re-export を削除、新型の re-export を追加
- `crates/tokio-http2/`: `shiguredo_http2::StreamId` を使用する全箇所を書き換え
- `crates/shiguredo_nghttp2/`: `StreamId = i32` は nghttp2 FFI 用であり変更不要
- `pbt/tests/prop_frame.rs`: `valid_stream_id()` strategy を `NonZeroStreamId` 生成に変更。
  stream_id=0 エラーテストは型レベルで構築不能になるため、削除または decoder レベルのテストに変換
- `pbt/tests/prop_connection.rs`: `client_stream_id()` strategy を `ClientStreamId` 生成に変更
- `pbt/tests/prop_event.rs`: strategy を新型に合わせて変更
- `tests/`, `fuzz/`, `examples/`: API 追従

## CHANGES.md エントリ

```
- [ADD] `ClientStreamId::from_static` / `ServerStreamId::from_static` を `const fn` で
  追加し、不正なリテラル ID をコンパイル時に検出可能にする
  - @担当者
- [CHANGE] `StreamId` を `u32` エイリアスから `enum StreamId { Connection, Client(_), Server(_) }`
  に変更し、奇偶ルールと接続制御 ID を型で表現する
  - @担当者
- [CHANGE] `CONNECTION_STREAM_ID` 定数を廃止し、`StreamId::Connection` に置き換える
  - @担当者
```

## 受け入れ条件

- `StreamId` が enum で定義され、`Connection` / `Client(ClientStreamId)` / `Server(ServerStreamId)` に分かれている
- `NonZeroStreamId` enum が定義され、`Client` / `Server` に分かれている
- `ClientStreamId::new` / `ServerStreamId::new` / `NonZeroStreamId::new` が `Result<Self, StreamIdError>` を返す
- `StreamIdError::ParityMismatch` が `got: u32` フィールドを持つ (issue 0029 と一致)
- `*::from_static` が `const fn` で実装され、不正リテラルでコンパイルエラーになる
- `StreamId` に `PartialOrd` / `Ord` が derive されていない
- `StreamId::from_wire` が wire 上の u32 を分類する
- `FrameHeader.stream_id` は `u32` のまま保持する
- `validate_stream_id_parity` が削除されている
- role ベースの偶奇検査が variant match で実現されている
- `CONNECTION_STREAM_ID` 定数が削除されている
- 既存の全テスト・PBT・fuzz が通る

## 依存

- [[0029-change-split-error-types]] (`StreamIdError` の設計方針)

## 関連

- [[0024-change-header-field-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]] (各フレーム型の構築 API への `NonZeroStreamId` 適用)
- [[0032-add-trybuild-compile-fail-tests]] (`from_static` の compile_fail テスト)
- issue 0023 §2 (`validate_stream_id_parity` の冗長な role match) は本 issue で解消
