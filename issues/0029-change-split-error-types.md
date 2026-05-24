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

`crate::error::Error` (既存) は **接続/ストリームエラー専用** に絞る。

```rust
pub struct Error {
    pub kind: ErrorKind,
    pub reason: String,
    pub location: &'static Location<'static>,
    pub backtrace: Backtrace,
}

pub enum ErrorKind {
    ConnectionError(ErrorCode),
    StreamError(ErrorCode),
    /// HPACK 符号化レベルのエラー (動的テーブルインデックス範囲外、整数オーバーフロー、
    /// Huffman 復号失敗等)。RFC 7541 由来の構造的エラーであり、構築時検査とは異なる。
    /// 受信側で検出され COMPRESSION_ERROR (接続エラー) に変換される。
    HpackError,
    // BufferTooShort / Incomplete / InvalidInput は削除 (次節参照)
}
```

### `BufferTooShort` / `Incomplete` / `InvalidInput` の移行先

`ErrorKind` から削除する 3 variant の移行先:

| 既存 variant | 移行先 | 根拠 |
|---|---|---|
| `BufferTooShort` | `DecodeError::BufferTooShort` (新設) | encoder/decoder/hpack 内部のバッファ操作エラー |
| `Incomplete` | `DecodeError::Incomplete` (新設) | ストリーミングデコード時の入力不足 |
| `InvalidInput` | 各ドメインエラー型の対応 variant | 構築時検査の個別エラーに分解 |

`DecodeError` はフレーム decoder / HPACK decoder が共通で使用する内部デコードエラー型:

```rust
// src/decode_error.rs (新規)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// バッファが必要なサイズに満たない
    BufferTooShort { required: usize, available: usize },
    /// 入力データが不足している (ストリーミングデコード時)
    Incomplete,
}
```

既存メソッドの移行:
- `Error::buffer_too_short()` → `DecodeError::BufferTooShort { ... }` を返す
- `Error::incomplete()` → `DecodeError::Incomplete` を返す
- `Error::invalid_input(reason)` → 各構築点のドメインエラー型に置き換え
- `Error::check_buffer_size(required, buf)` → `DecodeError::check_buffer_size(required, buf)` に移動

`FrameDecoder::decode()` の戻り値型は `Result<Option<Frame>, DecodeError>` に変更する。
接続エラーへの昇格が必要な場合は `From<DecodeError> for Error` で変換する。

### 新規ドメインエラー型

各構築点ごとに `#[non_exhaustive] enum` で定義。可能な限り `Copy + Clone + PartialEq + Eq` を
満たす軽量型とし、`Backtrace` は持たない。ただし `HeaderFieldError` のように違反値を
`Vec<u8>` で保持する型は `Copy` を導出できないため、`Clone + PartialEq + Eq` のみとする。

#### `HeaderFieldError` (issue 0024 で定義)

issue 0024 の設計に準拠する。0029 では定義を重複させず、0024 を参照する。
variant 一覧: `EmptyFieldName`, `UppercaseFieldName`, `InvalidFieldNameByte`,
`InvalidFieldValueByte`, `FieldValueLeadingOrTrailingWhitespace`, `UnknownPseudoHeader`,
`InvalidPseudoHeaderValue` (計 7 variant)。

#### `SettingError` (既存 `SettingsError` をリネーム・再設計)

既存の `SettingsError` (複数形、5 variant) を `SettingError` (単数形) にリネームし、
issue 0026 の設計に従って再構成する。

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingError {
    EnablePushNotBoolean { value: u32 },
    InitialWindowSizeOutOfRange { value: u32, max: u32 },
    MaxFrameSizeOutOfRange { value: u32, min: u32, max: u32 },
    EnableConnectProtocolNotBoolean { value: u32 },
    NoRfc7540PrioritiesNotBoolean { value: u32 },
}
```

#### `FrameError`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    ZeroStreamIdNotAllowed { frame_type: FrameType },
    NonZeroStreamIdNotAllowed { frame_type: FrameType, stream_id: u32 },
    ZeroWindowIncrement,
    WindowIncrementOutOfRange { value: u32 },
    InvalidWeight { value: u16 },
    PaddingExceedsPayload { padding: u8, payload_len: usize },
    LastStreamIdOutOfRange { value: u32 },
}
```

#### `StreamIdError` (issue 0025 で定義)

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamIdError {
    Reserved,
    ParityMismatch { expected: Parity, got: u32 },
    OutOfRange { value: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity { Odd, Even }
```

#### `LimitsError`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsError {
    WebtransportRequiresConnectProtocol,
}
```

注: `ConnectionWindowSmallerThanInitial` は issue 0028 で「許容する (RFC に禁止規定なし)」と
判断されたため、variant として定義しない。

#### `SendError`

`SendError` は本 issue では導入せず、[[0027-change-frame-construct-time-validation]] と
合わせて設計・導入する。フレーム単位の送信 API (`Connection::send_*`) と密結合する
ため、`FrameError` と同時に設計したほうが variant の境界が明確になる。

本 issue 0029 では `Error::invalid_input` 6 箇所 (`src/connection/mod.rs`) を、
既存の `Error::protocol_error` / `Error::stream_error` で代替する。
`SendError` 導入時に、これらの呼び出しは新型に置き換わる。

### 既存 `Error` への昇格

ドメインエラーは必要に応じて `Error` (接続レベル) に昇格させる `From` 実装を持つ。

```rust
impl From<FrameError> for Error {
    fn from(e: FrameError) -> Self {
        match e {
            // stream_id=0 系 → PROTOCOL_ERROR (接続エラー)
            FrameError::ZeroStreamIdNotAllowed { .. } => {
                Error::connection_error(ErrorCode::ProtocolError, e.to_string())
            }
            // increment=0 → PROTOCOL_ERROR (接続レベル or ストリームレベル、呼び出し元で判別)
            FrameError::ZeroWindowIncrement => {
                Error::connection_error(ErrorCode::ProtocolError, e.to_string())
            }
            // ...
        }
    }
}

impl From<DecodeError> for Error {
    fn from(e: DecodeError) -> Self {
        match e {
            DecodeError::BufferTooShort { .. } => {
                Error::connection_error(ErrorCode::FrameSizeError, e.to_string())
            }
            DecodeError::Incomplete => {
                Error::connection_error(ErrorCode::FrameSizeError, "incomplete frame")
            }
        }
    }
}
```

### `Backtrace` の opt-in 化

本 issue のスコープから除外し、別 issue で扱う。理由: feature flag 設計 (名前、default on/off、
Cargo.toml 変更、Error 構造体の条件付きフィールド定義) は独立した設計判断であり、
エラー型分割とは直交する。

### `ValidationError` の扱い

`src/validation.rs` の `ValidationError` は維持する。issue 0024 で定義された移行表に従い、
個別フィールド値検査に対応する variant (`InvalidHeaderName`, `InvalidHeaderValue`,
`InvalidMethodValue` 等) は `HeaderFieldError` に移行し、リスト整合性検査に対応する
variant は `ValidationError` に残す。`ValidationError` 自体は `Error` に変換する経路
(`malformed_error()`) を維持する。

## 影響範囲

- `src/error.rs`: `ErrorKind` から `BufferTooShort` / `Incomplete` / `InvalidInput` を削除。
  `Error::buffer_too_short()` / `Error::incomplete()` / `Error::invalid_input()` /
  `Error::check_buffer_size()` を削除
- `src/decode_error.rs` (新規): `DecodeError` 定義
- `src/hpack/error.rs` (新規): `HeaderFieldError` 定義 (issue 0024 と連携)
- `src/frame/error.rs` (新規): `FrameError` 定義
- `src/settings.rs`: `SettingsError` → `SettingError` にリネーム、variant 再構成
- `src/limits.rs`: `LimitsError` 追加
- `src/connection/mod.rs`: `Error::invalid_input` 6 箇所を `Error::protocol_error` /
  `Error::stream_error(ErrorCode::RefusedStream)` 等に置き換え (`SendError` 導入は 0027 へ)
- `src/frame/decoder.rs`: `Error::buffer_too_short()` → `DecodeError::BufferTooShort`、
  `Error::incomplete()` → `DecodeError::Incomplete` に置き換え。
  `FrameDecoder::decode()` の戻り値型を変更
- `src/hpack/decoder.rs`: HPACK デコードエラーの移行
- `src/hpack/huffman.rs`: `Error::buffer_too_short()` → `DecodeError::BufferTooShort` に置き換え
- `src/hpack/integer.rs`: `Error::buffer_too_short()` → `DecodeError::BufferTooShort` に置き換え
- `src/frame/encoder.rs`: `Error::check_buffer_size()` → `DecodeError::check_buffer_size()` に移行
- `src/validation.rs`: 移行対象 variant の削除 (0024 と連携)
- `src/lib.rs`: 新規ドメインエラー型の `pub use` 追加
- 全テスト・PBT・fuzz: 新エラー型でのアサーション書き換え

## CHANGES.md エントリ

```
- [CHANGE] 構築時エラーを `HeaderFieldError` / `SettingError` / `LimitsError` に
  分割し、各 variant が違反値を構造化フィールドで保持するように変更する
  (`FrameError` / `StreamIdError` / `SendError` は別 issue で導入)
  - @担当者
- [CHANGE] `ErrorKind::InvalidInput` / `BufferTooShort` / `Incomplete` を削除し、
  `DecodeError` 型および各ドメインエラー型に置き換える
  - @担当者
- [CHANGE] `SettingsError` を `SettingError` にリネームする
  - @担当者
```

## 受け入れ条件

- 各ドメインエラー型が `#[non_exhaustive]` で定義され、各 variant が違反値を保持している
- `DecodeError` が定義され、`BufferTooShort` / `Incomplete` の移行先として機能している
- 既存 `Error` の責務が「接続/ストリームエラー」「HPACK エラー」に絞られている
- `ErrorKind::BufferTooShort` / `Incomplete` / `InvalidInput` が削除されている
- `SettingsError` が `SettingError` にリネームされている
- `From<DomainError> for Error` / `From<DecodeError> for Error` が必要箇所で実装されている
- 上位アプリが文字列マッチではなく `match e` で失敗種別を分岐できる
- `src/lib.rs` で全ドメインエラー型が re-export されている
- 既存の全テスト・PBT・fuzz が通る

## 関連

- [[0024-change-header-field-construct-time-validation]] (`HeaderFieldError` 定義)
- [[0025-change-stream-id-newtype]] (`StreamIdError` 定義)
- [[0026-change-setting-construct-time-validation]] (`SettingError` 定義)
- [[0027-change-frame-construct-time-validation]] (`FrameError` 定義)
- [[0028-change-limits-builder-result]] (`LimitsError` 定義)
