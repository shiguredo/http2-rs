# Setting / SettingsFrame を構築時検査型に変更する

Created: 2026-05-23
Completed: 2026-05-24
Model: Opus 4.7
Branch: feature/change-phase2-construct-time-validation

## 概要

`Setting` (1 つのパラメータ) と `SettingsFrame` (パラメータの集合) を構築時検査型に作り直す。
現状は `Setting { id: u16, value: u32 }` の構造体で、不正値 (範囲外) を持った
`Setting` を構築できる。検査は `Settings::apply()` や `Connection::handle_settings` で
事後実施されている。

`Setting` を既知パラメータの enum に変更し、値範囲を型で制約する。リテラル定数向けの
`const fn from_static` を各制約型に提供する。

## 背景

現状の問題:

- `Setting::new(0x04, u32::MAX)` のように `INITIAL_WINDOW_SIZE > 2^31 - 1`
  (RFC 9113 §6.5.2 FLOW_CONTROL_ERROR) を構築可能
- `Setting::new(0x05, 100)` のように `MAX_FRAME_SIZE < 16384` (PROTOCOL_ERROR) を構築可能
- `Setting::new(0x02, 2)` のように `ENABLE_PUSH` が 0/1 以外 (PROTOCOL_ERROR) を構築可能
- `Setting::new(0x08, 2)` のように `ENABLE_CONNECT_PROTOCOL` が 0/1 以外を構築可能
- 不正値が `Connection::handle_settings` まで到達した時点で初めて検出される
- `SettingId` enum (src/settings.rs:69-107) が既に ID 解決を担っているが、
  `Setting` 構造体はそれを活用せず `id: u16` のままである

## 根拠

RFC 9113 §6.5.2 と関連仕様:

- `SETTINGS_HEADER_TABLE_SIZE (0x01)`: 制約なし (u32 全域)
- `SETTINGS_ENABLE_PUSH (0x02)`: 0 または 1 のみ、それ以外は PROTOCOL_ERROR
- `SETTINGS_MAX_CONCURRENT_STREAMS (0x03)`: 制約なし
- `SETTINGS_INITIAL_WINDOW_SIZE (0x04)`: 2^31 - 1 を超えると FLOW_CONTROL_ERROR
- `SETTINGS_MAX_FRAME_SIZE (0x05)`: 2^14 (16384) 以上 2^24 - 1 (16777215) 以下、
  範囲外は PROTOCOL_ERROR
- `SETTINGS_MAX_HEADER_LIST_SIZE (0x06)`: 制約なし
- `SETTINGS_ENABLE_CONNECT_PROTOCOL (0x08)`: RFC 8441 §3、0 または 1 のみ
- `SETTINGS_NO_RFC7540_PRIORITIES (0x09)`: RFC 9218 §2.1、0 または 1 のみ
- WebTransport SETTINGS (0x2b61-0x2b66): draft-ietf-webtrans-http2-14 §11.2、全て u32 値

## 設計

### `Setting` enum (既存の `Setting` 構造体と `SettingId` enum を統合)

既存の `SettingId` enum は `Setting` enum に統合され、不要になるため削除する。
`Setting` enum の各 variant が ID と型安全な値を持つ。

```rust
/// 既知の SETTINGS パラメータ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Setting {
    HeaderTableSize(u32),
    EnablePush(bool),
    MaxConcurrentStreams(u32),
    InitialWindowSize(WindowSize),
    MaxFrameSize(MaxFrameSize),
    MaxHeaderListSize(u32),
    EnableConnectProtocol(bool),
    NoRfc7540Priorities(bool),
    WtInitialMaxData(u32),
    WtInitialMaxStreamDataUni(u32),
    WtInitialMaxStreamDataBidiLocal(u32),
    WtInitialMaxStreamsUni(u32),
    WtInitialMaxStreamsBidi(u32),
    WtInitialMaxStreamDataBidiRemote(u32),
    /// 未知の SETTINGS パラメータ (RFC 9113 §6.5.2: MUST ignore)
    Unknown { id: u16, value: u32 },
}

impl Setting {
    /// wire 上の (id, value) ペアから構築する。
    /// 既知パラメータの値が範囲外の場合は Err(SettingError) を返す。
    /// 未知の ID の場合は Ok(Setting::Unknown { id, value }) を返す
    /// (RFC 9113 §6.5.2: 未知パラメータは無視 MUST)。
    pub fn from_wire(id: u16, value: u32) -> Result<Self, SettingError>;

    /// wire 上の (id, value) ペアに変換する
    pub const fn as_wire(self) -> (u16, u32);
}
```

WebTransport variant 名は既存の `SettingId` enum と完全に一致させる。
wire 上の値は全て `u32` (RFC 9113 §6.5.1) であるため、`u64` は使用しない。

### 制約付き型

```rust
/// SETTINGS_INITIAL_WINDOW_SIZE 用の制約付き型 (0..=2^31-1)
/// connection_window_size にも流用する (issue 0028)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowSize(u32);

impl WindowSize {
    pub const MAX: u32 = (1 << 31) - 1;
    pub fn new(size: u32) -> Result<Self, SettingError>;
    pub const fn from_static(size: u32) -> Self;  // const eval で panic
    pub const fn get(self) -> u32;
}

/// SETTINGS_MAX_FRAME_SIZE 用の制約付き型 (16384..=16777215)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaxFrameSize(u32);

impl MaxFrameSize {
    pub const MIN: u32 = 1 << 14;        // 16384
    pub const MAX: u32 = (1 << 24) - 1;  // 16777215
    pub fn new(size: u32) -> Result<Self, SettingError>;
    pub const fn from_static(size: u32) -> Self;
    pub const fn get(self) -> u32;
}
```

### `SettingError` (issue 0029 の設計に準拠)

`SettingError` の variant 定義は issue 0029 に従う。本 issue では `from_wire` / `WindowSize::new` /
`MaxFrameSize::new` が返すエラーとして使用する。

既存の `SettingsError` (複数形、src/settings.rs:337-349) は `SettingError` (単数形) に
リネームして統合する。

### `Settings` struct の WtInitialSettings 展開

`WtInitialSettings` 構造体を削除し、`Settings` struct に個別フィールドとして展開する:

```rust
pub struct Settings {
    pub header_table_size: u32,
    pub enable_push: bool,
    pub max_concurrent_streams: Option<u32>,
    pub initial_window_size: u32,
    pub max_frame_size: u32,
    pub max_header_list_size: Option<u32>,
    pub enable_connect_protocol: bool,
    pub no_rfc7540_priorities: bool,
    // WtInitialSettings から展開
    pub wt_initial_max_data: Option<u32>,
    pub wt_initial_max_stream_data_uni: Option<u32>,
    pub wt_initial_max_stream_data_bidi_local: Option<u32>,
    pub wt_initial_max_streams_uni: Option<u32>,
    pub wt_initial_max_streams_bidi: Option<u32>,
    pub wt_initial_max_stream_data_bidi_remote: Option<u32>,
}
```

`Settings::apply()` は `Setting` が検証済み (`from_wire` 済み) のため `-> ()` に変更する。
ただし以下の接続状態依存チェックは `apply()` の責務外であり、`Connection::handle_settings`
に残す:

- `ENABLE_PUSH=1` のサーバー → クライアント禁止 (RFC 9113 §8.4, role 依存)
- `NO_RFC7540_PRIORITIES` 変更不可 (RFC 9218 §2.1, 初回受信値依存)

`Settings::to_settings_list()` は `Setting` enum の各 variant を返すように全面書き換え。
`Connection::initiate()` 内の `SettingId` ベースのフィルタは `matches!(setting, Setting::EnablePush(_))`
に変更する。

### decoder のエラー変換

decoder で `Setting::from_wire(id, value)` が `Err(SettingError)` を返した場合、
直接 `Error` に変換する (`From<SettingError> for Error` を実装):

- `SettingError::InitialWindowSizeOutOfRange` → `FLOW_CONTROL_ERROR`
- その他 → `PROTOCOL_ERROR`

この変換は issue 0029 の `DecodeError` 導入前でも動作する。0029 実装後も変更不要。

未知 ID は `Ok(Setting::Unknown { id, value })` を返すため、decoder はエラーにしない。
`Connection::handle_settings` で `Setting::Unknown` を無視する。

### `SettingsFrame` の変更

```rust
impl SettingsFrame {
    pub fn new() -> Self;
    pub fn ack() -> Self;
    pub fn add(&mut self, setting: Setting);  // Setting が検証済みなので無検査
    pub fn from_settings(settings: impl IntoIterator<Item = Setting>) -> Self;
    pub fn settings(&self) -> &[Setting];
    pub fn is_ack(&self) -> bool;
    // 全フィールドを private 化
}
```

`SettingsFrame` の `settings` フィールドと `ack` フィールドを private 化し、アクセサを提供する。
`Setting` 自体が検証済みなので、`add` は `Result` を返さない (概要の「Result 化し」は誤り)。

`SettingsFrame` 全体としての制約 (例: ENABLE_CONNECT_PROTOCOL を一度 1 にした後で
0 に下げる禁止) は接続状態に依存するため、`Connection::send_settings` で検査する。

## 影響範囲

- `src/settings.rs`: `Setting` 構造体 → enum に全面書き換え。`SettingId` enum を削除。
  `SettingsError` を `SettingError` にリネーム・統合。`WindowSize` / `MaxFrameSize` 型を追加。
  `WtInitialSettings` 構造体を削除し、WT 関連値は `Settings` の個別フィールドに展開する。
  `Settings::apply()` は `Setting` が検証済み (from_wire 済み) のため `-> ()` に変更する
- `src/frame/mod.rs`: `SettingsFrame` のフィールド private 化、API 変更
- `src/frame/decoder.rs`: `Setting::from_wire` 経由でパース、エラーを接続エラーに変換
- `src/frame/encoder.rs`: `Setting::as_wire` 経由でエンコード
- `src/connection/mod.rs`: `handle_settings` の値検査を `Setting::from_wire` に統合。
  `Setting::Unknown` の無視処理
- `src/limits.rs`: `with_initial_window_size` 等が `WindowSize` / `MaxFrameSize` を直接受ける
  (panic → 型安全に移行)。詳細は issue 0028 で扱う
- `src/lib.rs`: `SettingId` の re-export を削除、`WindowSize` / `MaxFrameSize` / `SettingError`
  の re-export を追加
- `pbt/tests/prop_settings.rs`: `Setting::new(id, value)` → `Setting::from_wire(id, value)` への
  書き換え。strategy を新 enum に合わせて変更
- `pbt/tests/prop_frame.rs`: SETTINGS フレームの PBT を新 API に追従
- `pbt/tests/prop_connection.rs`: SETTINGS 受信テストを新 API に追従
- `tests/`, `fuzz/`, `examples/`: API 追従

## CHANGES.md エントリ

```
- [ADD] `Setting` 系の `const fn from_static` で不正リテラルをコンパイル時に検出可能にする
  - @担当者
- [CHANGE] `Setting` を `{ id: u16, value: u32 }` 構造体から既知パラメータの enum に変更し、
  値範囲を型で制約する
  - @担当者
- [CHANGE] `WindowSize` / `MaxFrameSize` 型を新設し、RFC 9113 §6.5.2 の値範囲制約を
  構築時に強制する
  - @担当者
- [CHANGE] `SettingId` enum を `Setting` enum に統合し削除する
  - @担当者
```

## 受け入れ条件

- `Setting` が既知パラメータの enum で定義され、各 variant のペイロード型が値範囲を表現している
- `SettingId` enum が削除されている
- `SettingsError` が `SettingError` にリネームされている
- WebTransport variant の型が全て `u32` である (wire フォーマットと一致)
- WebTransport variant 名が既存の `SettingId` と一致している
- `WindowSize::new` / `MaxFrameSize::new` が `Result<Self, SettingError>` を返す
- `*::from_static` が `const fn` で実装され、不正リテラルでコンパイルエラーになる
- `Setting::from_wire` が未知 ID を `Ok(Setting::Unknown { id, value })` で返す
- decoder は `Setting::from_wire` 経由で組み立て、不正値は接続エラーに変換される
- `SettingsFrame` のフィールドが private 化されている
- `Connection::handle_settings` の値検査ロジックが `Setting::from_wire` に統合されている
- 既存の全テスト・PBT・fuzz が通る

## 依存

- [[0029-change-split-error-types]] (`SettingError` の設計方針)

## 関連

- [[0024-change-header-field-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]]
- [[0028-change-limits-builder-result]] (`WindowSize` / `MaxFrameSize` を利用)
- [[0032-add-trybuild-compile-fail-tests]] (`from_static` の compile_fail テスト)

## 解決方法

以下の変更を実施した:

1. `Setting` を `struct { id: u16, value: u32 }` から既知パラメータの enum に変更。`Setting::from_wire(id, value) -> Result<Self, SettingError>` で wire 値から構築し、値範囲検査を実施。`Setting::as_wire() -> (u16, u32)` で wire 値に戻す
2. `SettingId` enum を `Setting` enum に統合し削除
3. `WtInitialSettings` 構造体を削除し、`Settings` / `Limits` の個別フィールド (`wt_initial_max_data` 等) に展開
4. `SettingsFrame` のフィールド (`ack`, `settings`) を private 化し、アクセサ (`is_ack()`, `settings()`) を追加。`add_setting` を `add` にリネーム。`from_settings` コンストラクタを追加
5. `Settings::apply()` の戻り値を `Result<(), SettingError>` から `()` に変更。`Setting` が `from_wire` で構築時検査済みのため、apply 時の値検査は不要
6. decoder (`src/frame/decoder.rs`) で `Setting::from_wire` を呼び出し、`SettingError` を接続エラーに変換 (`InitialWindowSizeOutOfRange` → `FLOW_CONTROL_ERROR`、他 → `PROTOCOL_ERROR`)
7. `Limits::with_webtransport` の引数を `WtInitialSettings` から個別の `Option<u32>` x 6 に変更
8. PBT に WebTransport variant を追加し、`prop_settings_roundtrip` を `enable_connect_protocol` / `no_rfc7540_priorities` / WebTransport フィールドを含むように拡充

### 設計上の注意点

- `Settings` 構造体のフィールドは `pub` のまま残している。フィールド直接代入で不正値を設定し `to_settings_list()` で panic する経路が存在するが、`Settings` のフィールド private 化は issue 0028 (Limits ビルダー Result 化) のスコープとして保留
- `with_webtransport` の 6 引数 `Option<u32>` は型安全性が低い (引数順序の取り違えを検出できない)。個別ビルダーへの分割も issue 0028 のスコープとして保留
