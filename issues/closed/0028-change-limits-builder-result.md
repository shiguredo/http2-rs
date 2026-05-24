# Limits ビルダーの panic を build() Result に置き換える

Created: 2026-05-23
Completed: 2026-05-24
Priority: High
Model: Opus 4.7

## 概要

`Limits` のビルダー (`with_initial_window_size`, `with_max_frame_size`,
`with_connection_window_size`) は現在、値が範囲外の場合に `assert!` で panic する。
これを `LimitsBuilder` + `build() -> Result<Limits, LimitsError>` に統一する。
リテラル定数向けに `build_static()` (`const fn`) も併設する。

## 背景

現状 (`src/limits.rs`):

```rust
pub fn with_initial_window_size(mut self, size: u32) -> Self {
    assert!(
        size <= MAX_INITIAL_WINDOW_SIZE,
        "initial window size must be <= {MAX_INITIAL_WINDOW_SIZE}"
    );
    self.initial_window_size = size;
    self
}
```

問題:

- ビルダー API が panic で失敗する設計は、利用者がランタイム値 (config ファイル, env var) を
  渡すユースケースでサーバープロセス全体を落とす可能性がある
- `Limits` の全フィールドが `pub` であるため、ビルダーを経由せず構造体リテラルで
  直接構築でき、範囲検査を迂回可能
- 既存テスト (`src/limits.rs:170-186`) に `#[should_panic]` が 3 件あり、
  panic ベースの設計に依存している

## 根拠

- RFC 9113 §6.5.2:
  - `SETTINGS_INITIAL_WINDOW_SIZE` の最大値は 2^31 - 1
  - `SETTINGS_MAX_FRAME_SIZE` は 2^14 (16384) 以上 2^24 - 1 以下
- RFC 9113 §6.9.1: フロー制御ウィンドウは 2^31 - 1 オクテットを超えてはならない (MUST NOT)
- RFC 9113 §6.9.2: 接続レベルフロー制御ウィンドウの初期値は 65,535 オクテット
- 値範囲制約はライブラリ呼び出し側で配布可能 (configuration からの値) なので、
  panic ではなく `Result` で返すべき

## 設計

### フィールドの private 化

`Limits` の全フィールドを private にし、getter を提供する:

```rust
pub struct Limits {
    max_concurrent_streams: Option<u32>,
    initial_window_size: WindowSize,
    max_frame_size: MaxFrameSize,
    max_header_list_size: Option<u32>,
    header_table_size: u32,
    connection_window_size: WindowSize,
    enable_connect_protocol: bool,
    no_rfc7540_priorities: bool,
    // WebTransport: 0026 で WtInitialSettings が削除される場合は個別フィールドに展開
    wt_initial_max_data: Option<u32>,
    wt_initial_max_stream_data_uni: Option<u32>,
    wt_initial_max_stream_data_bidi_local: Option<u32>,
    wt_initial_max_streams_uni: Option<u32>,
    wt_initial_max_streams_bidi: Option<u32>,
    wt_initial_max_stream_data_bidi_remote: Option<u32>,
}

impl Limits {
    pub fn max_concurrent_streams(&self) -> Option<u32>;
    pub fn initial_window_size(&self) -> WindowSize;
    pub fn max_frame_size(&self) -> MaxFrameSize;
    pub fn max_header_list_size(&self) -> Option<u32>;
    pub fn header_table_size(&self) -> u32;
    pub fn connection_window_size(&self) -> WindowSize;
    pub fn enable_connect_protocol(&self) -> bool;
    pub fn no_rfc7540_priorities(&self) -> bool;
    // WT getter 群...
}
```

`initial_window_size` と `connection_window_size` は `WindowSize` 型 (issue 0026)、
`max_frame_size` は `MaxFrameSize` 型 (issue 0026) を使用する。

### ビルダー API

```rust
#[derive(Debug, Clone)]
pub struct LimitsBuilder {
    max_concurrent_streams: Option<u32>,
    initial_window_size: WindowSize,
    max_frame_size: MaxFrameSize,
    max_header_list_size: Option<u32>,
    header_table_size: u32,
    connection_window_size: WindowSize,
    enable_connect_protocol: bool,
    no_rfc7540_priorities: bool,
    wt_initial_max_data: Option<u32>,
    wt_initial_max_stream_data_uni: Option<u32>,
    wt_initial_max_stream_data_bidi_local: Option<u32>,
    wt_initial_max_streams_uni: Option<u32>,
    wt_initial_max_streams_bidi: Option<u32>,
    wt_initial_max_stream_data_bidi_remote: Option<u32>,
}

impl Limits {
    pub const fn builder() -> LimitsBuilder;
}

impl LimitsBuilder {
    pub const fn max_concurrent_streams(mut self, max: Option<u32>) -> Self;
    pub const fn initial_window_size(mut self, size: WindowSize) -> Self;
    pub const fn max_frame_size(mut self, size: MaxFrameSize) -> Self;
    pub const fn max_header_list_size(mut self, size: Option<u32>) -> Self;
    pub const fn header_table_size(mut self, size: u32) -> Self;
    pub const fn connection_window_size(mut self, size: WindowSize) -> Self;
    pub const fn enable_connect_protocol(mut self, enable: bool) -> Self;
    pub const fn no_rfc7540_priorities(mut self, enable: bool) -> Self;
    // WT 個別メソッド
    pub const fn wt_initial_max_data(mut self, value: Option<u32>) -> Self;
    // ... 他 WT フィールドも同様

    /// 複合制約を検査して構築する
    pub fn build(self) -> Result<Limits, LimitsError>;

    /// const コンテキスト用。不正制約でコンパイル時 panic になる
    pub const fn build_static(self) -> Limits;
}
```

`WindowSize` / `MaxFrameSize` は issue 0026 で導入する範囲制約型を再利用する。
ビルダーの個別メソッドは値範囲検査を行わず、型システムに委ねる。
`LimitsBuilder` のデフォルト値は現行 `Limits::default()` と同一。

### `Limits::new()` と `Default for Limits` の扱い

- `Limits::new()` は削除する
- `impl Default for Limits` は維持する (`Limits::builder().build().unwrap()` と等価)
- `Default::default()` はデフォルト値で必ず成功するため、unwrap は安全

### 複合制約検査 (`build()` 時)

`build()` で検査する制約:

- WebTransport 関連フィールドが設定されている場合、`enable_connect_protocol = true` が
  必須 (draft-ietf-webtrans-http2-14 §11.1) → `LimitsError::WebtransportRequiresConnectProtocol`

`connection_window_size < initial_window_size` は許容する (RFC に禁止規定なし)。

### 既存テストの移行

`src/limits.rs` 内の `#[should_panic]` テスト 3 件:

- `test_invalid_initial_window_size` → 0026 で `WindowSize` 型制約導入後は `WindowSize::new` が
  `Err` を返すテストに変更。`tests/test_limits.rs` に移動
- `test_invalid_max_frame_size_too_small` → 同上、`MaxFrameSize::new` が `Err` を返すテスト
- `test_invalid_max_frame_size_too_large` → 同上

### 互換性

破壊的変更:
- `Limits::new()` 削除 → `Limits::builder().build()?` に置き換え
- `Limits::with_*` 削除 → `LimitsBuilder` 経由
- `Limits` フィールドが private → getter 経由
- チェイン API (`Limits::new().with_X(...)`) は使えなくなる

## 影響範囲

- `src/limits.rs`: `Limits` フィールド private 化、getter 追加、`LimitsBuilder` 導入、
  `Limits::new()` 削除、`#[should_panic]` テスト移動
- `src/connection/mod.rs`: `Connection::new` 内の `limits.field` → `limits.field()` に変更
  (9 箇所: `initial_window_size`, `max_frame_size`, `header_table_size`, `max_concurrent_streams`,
  `max_header_list_size`, `enable_connect_protocol`, `no_rfc7540_priorities`, `wt_initial`,
  `connection_window_size`)
- `tests/test_limits.rs` (新規): `#[should_panic]` テストを `Result::Err` テストに移行
- `pbt/tests/prop_connection.rs`: `Limits::default()` を使用しているため変更不要 (確認のみ)
- `crates/tokio-http2/`: `Limits` を使用するテスト・API の追従
- `fuzz/fuzz_targets/`: `Limits::new()` → `Limits::default()` に変更
- `examples/`: API 追従

## CHANGES.md エントリ

```
- [ADD] `LimitsBuilder::build_static` (`const fn`) を追加し、リテラル定数で構築する
  Limits の範囲違反をコンパイル時に検出可能にする
  - @担当者
- [CHANGE] `Limits::new()` / `with_*` を `LimitsBuilder` 経由に置き換え、範囲外値で
  panic していた挙動を `LimitsBuilder::build() -> Result` に変更する
  - @担当者
- [CHANGE] `Limits` のフィールドを private 化し、getter メソッドを提供する
  - @担当者
```

## 受け入れ条件

- `Limits::new()` / `Limits::with_*` メソッドが削除されている
- `Limits` の全フィールドが private で、getter 経由でのみアクセス可能
- `LimitsBuilder::build()` が `Result<Limits, LimitsError>` を返す
- `LimitsBuilder::build_static()` が `const fn` で実装されている
- `Default for Limits` が維持され、デフォルト値で構築可能
- `assert!` ベースの panic が全て削除されている
- 既存の `#[should_panic]` テストが `Result::Err` テストに移行されている
- `Connection::new` 内のフィールドアクセスが getter 経由に変更されている
- 既存の全テスト・PBT・fuzz が通る

## 解決方法

- `Limits` の全フィールドを private 化し、getter メソッド経由でのみアクセス可能にした
- `initial_window_size` / `connection_window_size` を `WindowSize` 型に、`max_frame_size` を `MaxFrameSize` 型に変更した
- `Limits::new()` / `with_*` メソッドを削除し、`LimitsBuilder` を導入した
- `LimitsBuilder::build() -> Result<Limits, LimitsError>` で複合制約検査 (WebTransport + connect_protocol) を実装した
- `LimitsBuilder::build_static()` を `const fn` で実装した
- `Default for Limits` を `Limits::builder().build().expect(...)` で維持した
- `Connection::new` 内のフィールドアクセスを getter 経由に変更した
- fuzz ターゲットの `Limits::new()` を `Limits::default()` に変更した
- examples (`wt_server`, `http2_server`, `http2_client`) を `LimitsBuilder` 経由に移行した
- tokio-http2 テスト (`client_server.rs`, `test_webtransport.rs`) を `LimitsBuilder` 経由に移行した
- 旧 `#[should_panic]` テスト 3 件は、値範囲検査が `WindowSize::new` / `MaxFrameSize::new` (issue 0026) に移動済みのため不要になり削除した

## 依存

- [[0026-change-setting-construct-time-validation]] (`WindowSize` / `MaxFrameSize` を提供。
  `WtInitialSettings` 削除に伴い WT メソッドは個別フィールドに展開する)
- [[0029-change-split-error-types]] (`LimitsError`)
