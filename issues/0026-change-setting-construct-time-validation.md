# Setting / SettingsFrame を構築時検査型に変更する

Created: 2026-05-23
Model: Opus 4.7

## 概要

`Setting` (1 つのパラメータ) と `SettingsFrame` (パラメータの集合) を構築時検査型に作り直す。
現状は `Setting { id: u16, value: u32 }` の単純な構造体で、不正値 (範囲外) を持った
`Setting` を構築できる。検査は送信時の `Connection::handle_settings` や
`Connection::send_frame` で事後実施されている。

`Setting::new` / `SettingsFrame::add_setting` を `Result` 化し、リテラル定数向けの
`Setting::from_static` (`const fn`) を提供する。

## 背景

現状の問題:

- `SettingsFrame::add_setting(Setting { id: 0x4, value: u32::MAX })` のように
  `INITIAL_WINDOW_SIZE > 2^31 - 1` (RFC 9113 §6.5.2 FLOW_CONTROL_ERROR) を構築可能
- `Setting { id: 0x5, value: 100 }` のように `MAX_FRAME_SIZE < 16384` (PROTOCOL_ERROR) を構築可能
- `Setting { id: 0x2, value: 2 }` のように `ENABLE_PUSH` が 0/1 以外 (PROTOCOL_ERROR) を構築可能
- `Setting { id: 0x8, value: 2 }` のように `SETTINGS_ENABLE_CONNECT_PROTOCOL` (RFC 8441) が
  0/1 以外を構築可能
- 不正値が `Connection::send_settings` まで到達した時点で初めて検出される

## 根拠

RFC 9113 §6.5.2 と関連仕様:

- `SETTINGS_HEADER_TABLE_SIZE (0x01)`: 制約なし (u32 全域)
- `SETTINGS_ENABLE_PUSH (0x02)`: 0 または 1 のみ、それ以外は PROTOCOL_ERROR
- `SETTINGS_MAX_CONCURRENT_STREAMS (0x03)`: 制約なし
- `SETTINGS_INITIAL_WINDOW_SIZE (0x04)`: 2^31 - 1 を超えると FLOW_CONTROL_ERROR
- `SETTINGS_MAX_FRAME_SIZE (0x05)`: 2^14 (16384) 以上 2^24 - 1 (16777215) 以下、
  範囲外は PROTOCOL_ERROR
- `SETTINGS_MAX_HEADER_LIST_SIZE (0x06)`: 制約なし
- `SETTINGS_ENABLE_CONNECT_PROTOCOL (0x08)`: RFC 8441 §3、0 または 1 のみ、サーバーから 1 受信後
  クライアントが 0 を送信すると PROTOCOL_ERROR
- `SETTINGS_NO_RFC7540_PRIORITIES (0x09)`: RFC 9218 §2.1、0 または 1 のみ
- WebTransport SETTINGS (0x2b61 - 0x2b66): draft-ietf-webtrans-http2-14 §11.2

## 設計

### 型定義

```rust
/// 既知の SETTINGS パラメータ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Setting {
    HeaderTableSize(u32),
    EnablePush(bool),
    MaxConcurrentStreams(u32),
    InitialWindowSize(WindowSize),       // 範囲を型で制約
    MaxFrameSize(MaxFrameSize),          // 範囲を型で制約
    MaxHeaderListSize(u32),
    EnableConnectProtocol(bool),         // RFC 8441
    NoRfc7540Priorities(bool),           // RFC 9218
    WtMaxSessions(u32),                  // WebTransport
    WtInitialMaxData(u64),
    WtInitialMaxStreamsBidi(u64),
    WtInitialMaxStreamsUni(u64),
    WtInitialMaxStreamDataBidi(u64),
    WtInitialMaxStreamDataUni(u64),
    /// 未知の SETTINGS パラメータ (RFC 9113 §6.5.2: 無視する)
    Unknown { id: u16, value: u32 },
}

impl Setting {
    pub fn from_wire(id: u16, value: u32) -> Result<Self, SettingError>;
    pub const fn as_wire(self) -> (u16, u32);
}

/// SETTINGS_INITIAL_WINDOW_SIZE 用の制約付き型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize(u32);

impl WindowSize {
    pub const MAX: u32 = (1 << 31) - 1;
    pub fn new(size: u32) -> Result<Self, SettingError>;
    pub const fn from_static(size: u32) -> Self;  // const eval で panic
    pub const fn get(self) -> u32;
}

/// SETTINGS_MAX_FRAME_SIZE 用の制約付き型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaxFrameSize(u32);

impl MaxFrameSize {
    pub const MIN: u32 = 1 << 14;        // 16384
    pub const MAX: u32 = (1 << 24) - 1;  // 16777215
    pub fn new(size: u32) -> Result<Self, SettingError>;
    pub const fn from_static(size: u32) -> Self;
    pub const fn get(self) -> u32;
}
```

### コンパイル時検査の例

```rust
// OK
const WINDOW: WindowSize = WindowSize::from_static(65535);
const FRAME: MaxFrameSize = MaxFrameSize::from_static(16384);
const SETTING: Setting = Setting::InitialWindowSize(WINDOW);

// NG: コンパイル時に "window size exceeds 2^31 - 1" で fail
const BAD_WINDOW: WindowSize = WindowSize::from_static(u32::MAX);

// NG: コンパイル時に "max frame size must be 16384..=16777215" で fail
const BAD_FRAME: MaxFrameSize = MaxFrameSize::from_static(1024);
```

### `SettingsFrame::add_setting` の扱い

```rust
impl SettingsFrame {
    pub fn new() -> Self;
    pub fn add(&mut self, setting: Setting);  // 既に検証済みなので無検査
    pub fn from_settings(settings: impl IntoIterator<Item = Setting>) -> Self;
}
```

`Setting` 自体が検証済みなので、`SettingsFrame::add` は値検査不要。
ただし、`SettingsFrame` 全体としての制約 (例: ENABLE_CONNECT_PROTOCOL を一度 1 にした後で
0 に下げる禁止) は接続状態に依存するため、`Connection::send_settings` で検査する。

## 影響範囲

- `src/settings.rs`: 型定義の全面書き換え
- `src/frame/mod.rs`: `SettingsFrame` API 変更
- `src/frame/decoder.rs`: `Setting::from_wire` 経由でパース
- `src/frame/encoder.rs`: `Setting::as_wire` 経由でエンコード
- `src/connection/mod.rs`: SETTINGS 受信処理の値検査を `Setting::from_wire` に統合
- `src/limits.rs`: `Limits::with_initial_window_size` 等が `WindowSize` / `MaxFrameSize` を直接受ける
- `tests/`, `pbt/`, `fuzz/`, `examples/`: API 追従

## CHANGES.md エントリ

```
- [CHANGE] `Setting` を `{ id: u16, value: u32 }` 構造体から既知パラメータの enum に変更し、
  値範囲を型で制約する
- [CHANGE] `WindowSize` / `MaxFrameSize` 型を新設し、RFC 9113 §6.5.2 の値範囲制約を構築時に強制する
- [ADD] `Setting` 系の `const fn from_static` で不正リテラルをコンパイル時に検出可能にする
```

## 受け入れ条件

- `Setting` が既知パラメータの enum で定義され、各 variant のペイロード型が値範囲を表現している
- `WindowSize::new` / `MaxFrameSize::new` が `Result<Self, SettingError>` を返す
- `*::from_static` が `const fn` で実装され、不正リテラルでコンパイルエラーになる
- decoder は `Setting::from_wire` 経由で組み立て、不正値は接続エラーに変換される
- `Connection::handle_settings` の値検査ロジックが `Setting::from_wire` に統合されている
- 既存の全テスト・PBT・fuzz が通る

## 関連

- [[0024-change-header-field-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]]
- [[0028-change-limits-builder-result]]
- [[0029-change-split-error-types]]
