# エラー型を構築点ごとのドメイン特化型に分割する

Created: 2026-05-23
Model: Opus 4.7

## 概要

現状の `Error { kind: ErrorKind, reason: String, location, backtrace }` 1 系統では、
構築時検査の各失敗ケースを構造化情報として返せない (`String reason` 文字列でしか区別できない)。
構築点ごとに専用エラー型を分割し、`#[non_exhaustive] enum` の variant に各失敗ケースの
コンテキスト値を保持する。

接続エラー / ストリームエラーを表す既存 `Error` は「接続状態に関わる送受信エラー」専用に絞り、
構築時エラーは別系統にする。

## 背景

現状の `Error` は接続/ストリームエラーと「構築時の入力不正」を同じ型で扱っており、
以下の問題がある:

- `Error::with_reason(ErrorKind::InvalidInput, "uppercase field name")` のように、
  失敗理由が文字列に潰される
- 上位コード (アプリケーション層) が「どのフィールドが不正だったか」を構造化情報として
  取り出せず、文字列パターンマッチを強いられる
- メトリクス分類 (どの種類の構築エラーが多発しているか) もできない
- `Backtrace` を常に持つ設計は、構築時エラーには重すぎる (短命で大量発生する可能性がある)

shiguredo_http11 では `EncodeError` を 23 variant に分割し、それぞれ違反値を構造化フィールドで
保持している。同じ方針を HTTP/2 にも適用する。

## 根拠

- 構築時エラーは「呼び出し側が即座にハンドリングする」前提なので、構造化情報が必須
- 接続エラー (リモート起因のプロトコル違反) は「ログ・GOAWAY・接続切断」につながるので、
  Backtrace つきの重い型でも妥当
- エラー型の分離により、それぞれに適した派生 trait (Copy, Clone, PartialEq, Hash) を選択できる
- shiguredo_http11 が同じ分離で堅牢化に成功しており、HTTP/2 でも同型を踏襲する

## 設計

### 既存型の責務再定義

`crate::error::Error` (既存) は **接続/ストリームエラー専用**に絞る。

```rust
// 役割: リモートが起こしたプロトコル違反 + ライブラリ内部の到達不能状態
pub struct Error {
    pub kind: ErrorKind,
    pub reason: String,
    pub location: &'static Location<'static>,
    pub backtrace: Backtrace,
}

pub enum ErrorKind {
    ConnectionError(ErrorCode),
    StreamError(ErrorCode),
    HpackError,
    // BufferTooShort / Incomplete / InvalidInput は移譲先のドメインエラーへ
}
```

### 新規ドメインエラー型

各構築点ごとに `#[non_exhaustive] enum` で定義。すべて `Copy + Clone + PartialEq + Eq` を満たす
軽量型とし、`Backtrace` は持たない。

```rust
// src/hpack/error.rs
#[non_exhaustive]
pub enum HeaderFieldError {
    EmptyFieldName,
    UppercaseFieldName { /* ... */ },
    InvalidFieldNameByte { /* ... */ },
    InvalidFieldValueByte { /* ... */ },
    UnknownPseudoHeader { /* ... */ },
    InvalidPseudoHeaderValue { /* ... */ },
}

// src/settings.rs
#[non_exhaustive]
pub enum SettingError {
    EnablePushNotBoolean { value: u32 },
    InitialWindowSizeOutOfRange { value: u32, max: u32 },
    MaxFrameSizeOutOfRange { value: u32, min: u32, max: u32 },
    EnableConnectProtocolNotBoolean { value: u32 },
    NoRfc7540PrioritiesNotBoolean { value: u32 },
    // ...
}

// src/frame/error.rs
#[non_exhaustive]
pub enum FrameError {
    ZeroStreamIdNotAllowed { frame_type: FrameType },
    NonZeroStreamIdNotAllowed { frame_type: FrameType, stream_id: u32 },
    ZeroWindowIncrement,
    WindowIncrementOutOfRange { value: u32 },
    InvalidWeight { value: u16 },
    PaddingExceedsPayload { padding: u8, payload_len: usize },
    LastStreamIdOutOfRange { value: u32 },
}

// src/stream/error.rs (StreamId 系)
#[non_exhaustive]
pub enum StreamIdError {
    Reserved,                          // id = 0
    ParityMismatch { expected: Parity, got: u32 },
    OutOfRange { value: u32 },         // >= 2^31
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity { Odd, Even }

// src/limits.rs
#[non_exhaustive]
pub enum LimitsError {
    WebtransportRequiresConnectProtocol,
    ConnectionWindowSmallerThanInitial { connection: u32, initial: u32 },
    // ...
}

// src/connection/error.rs (送信 API のエラー)
#[non_exhaustive]
pub enum SendError {
    ConnectionClosed,
    GoawaySent,
    StreamNotOpen { stream_id: u32 },
    FlowControlExhausted,
    HeaderListTooLarge { actual: usize, limit: u32 },
}
```

### 既存 `Error` への昇格

ドメインエラーは必要に応じて `Error` (接続レベル) に昇格させる `From` 実装を持つ。
例えば decoder で `FrameError::ZeroStreamIdNotAllowed` が出たら接続エラーに変換する。

```rust
impl From<FrameError> for Error {
    fn from(e: FrameError) -> Self {
        // 接続レベルか否かを variant ごとに判別して変換
    }
}
```

### `Backtrace` の opt-in 化

既存 `Error::backtrace` は常時取得する設計だが、`#[cfg(feature = "backtrace")]` で
opt-in に変更する。デバッグビルドのみ取得するのも選択肢。

## 影響範囲

- `src/error.rs`: `ErrorKind` から `BufferTooShort` / `Incomplete` / `InvalidInput` を削除
- 新規ファイル: `src/hpack/error.rs` (`HeaderFieldError`), `src/frame/error.rs` (`FrameError`),
  `src/stream/error.rs` (`StreamIdError`)
- 既存ファイルへの追加: `src/settings.rs` (`SettingError`), `src/limits.rs` (`LimitsError`)
- `src/connection/`: `SendError` 追加
- 全テスト・PBT・fuzz: 新エラー型でのアサーション書き換え

## CHANGES.md エントリ

```
- [CHANGE] 構築時エラーを `HeaderFieldError` / `SettingError` / `FrameError` /
  `StreamIdError` / `LimitsError` / `SendError` に分割し、各 variant が違反値を
  構造化フィールドで保持するように変更する
- [CHANGE] `Error::ErrorKind::InvalidInput` / `BufferTooShort` / `Incomplete` を削除し、
  対応する箇所をドメインエラー型に置き換える
```

## 受け入れ条件

- 各ドメインエラー型が `#[non_exhaustive]` で定義され、各 variant が違反値を保持している
- 既存 `Error` の責務が「接続/ストリームエラー」「HPACK エラー」に絞られている
- `From<DomainError> for Error` が必要箇所で実装されている
- 上位アプリが文字列マッチではなく `match e` で失敗種別を分岐できる
- 既存の全テスト・PBT・fuzz が通る

## 関連

- [[0024-change-header-field-construct-time-validation]]
- [[0025-change-stream-id-newtype]]
- [[0026-change-setting-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]]
- [[0028-change-limits-builder-result]]
