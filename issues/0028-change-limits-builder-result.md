# Limits ビルダーの panic を build() Result に置き換える

Created: 2026-05-23
Model: Opus 4.7

## 概要

`Limits` のビルダー (`with_initial_window_size`, `with_max_frame_size`,
`with_connection_window_size`) は現在、値が範囲外の場合に `assert!` で panic する。
これを `Result` 返却に統一し、最終的な `build() -> Result<Limits, LimitsError>` で
全制約を検査する形に変更する。リテラル定数向けに `const fn` ベースの構築 API も併設する。

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
  渡すユースケースで握り潰しが必要になる
- ライブラリの不変条件違反を panic で通知するのは、ライブラリ呼び出しを `catch_unwind` で
  囲うサーバーが少ないため、サーバープロセス全体を落とす可能性がある
- 一方で、テストや内部定数構築では `?` を書きたくないため `const fn` で「不正リテラルは
  コンパイル時に弾く」経路も欲しい

## 根拠

- RFC 9113 §6.5.2:
  - `SETTINGS_INITIAL_WINDOW_SIZE` の最大値は 2^31 - 1
  - `SETTINGS_MAX_FRAME_SIZE` は 2^14 (16384) 以上 2^24 - 1 以下
- 値範囲制約はライブラリ呼び出し側で配布可能 (configuration からの値) なので、
  panic ではなく `Result` で返すべき
- shiguredo_http11 では `DecoderLimits::unlimited()` のような明示的なコンストラクタを提供し、
  通常パスは `Default::default()` で構築する設計を採用している。`with_*` で範囲検査を要する
  値は存在しない

## 設計

### ビルダー API

```rust
pub struct Limits { /* ... */ }

#[derive(Debug, Clone)]
pub struct LimitsBuilder { /* 中間状態 */ }

impl Limits {
    pub const fn builder() -> LimitsBuilder;
    pub const fn default_const() -> Self;  // const fn 版 default
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
    pub const fn webtransport(mut self, wt: WtInitialSettings) -> Self;

    /// 範囲制約と整合性を検査して構築する
    pub fn build(self) -> Result<Limits, LimitsError>;

    /// const コンテキスト用。不正リテラルでコンパイル時 panic になる
    pub const fn build_static(self) -> Limits;
}
```

`WindowSize` / `MaxFrameSize` は issue 0026 で導入する範囲制約型を再利用する。これにより
ビルダー自体は値範囲検査を行わず、型システムに委ねる。

### 整合性検査 (build 時)

ビルダーの個別 `with_*` では検査しない複合制約を `build()` で検査する。例:

- `connection_window_size < initial_window_size` を許すか禁止するかの判断
- WebTransport 有効化時に `enable_connect_protocol = true` が必須
  (draft-ietf-webtrans-http2-14 §11.1)
- `max_header_list_size` が極端に小さい場合の警告 (エラーではなく構成的な不整合)

### コンパイル時検査の例

```rust
// 全て const コンテキスト、不正値はコンパイルエラー
const LIMITS: Limits = Limits::builder()
    .initial_window_size(WindowSize::from_static(65535))
    .max_frame_size(MaxFrameSize::from_static(16384))
    .build_static();
```

### 互換性

破壊的変更。`with_*` 系メソッドは削除して `LimitsBuilder` 経由に統一する。
チェイン API (`Limits::new().with_X(...).with_Y(...)`) は使えなくなり、
`Limits::builder().X(...).Y(...).build()?` の形になる。

## 影響範囲

- `src/limits.rs`: API 全面書き換え
- `src/connection/mod.rs`: `Connection::new(role, limits)` の呼び出し側 (テスト含む) が
  `?` を必要とする
- `tests/`, `pbt/`, `fuzz/`, `examples/`: API 追従

## CHANGES.md エントリ

```
- [CHANGE] `Limits::with_*` を `LimitsBuilder` 経由に置き換え、範囲外値で panic していた
  挙動を `LimitsBuilder::build() -> Result` に変更する
- [ADD] `LimitsBuilder::build_static` (`const fn`) を追加し、リテラル定数で構築する
  Limits の範囲違反をコンパイル時に検出可能にする
```

## 受け入れ条件

- `Limits::with_*` メソッドが削除され、`LimitsBuilder` 経由でのみ構築可能
- `LimitsBuilder::build()` が `Result<Limits, LimitsError>` を返す
- `LimitsBuilder::build_static()` が `const fn` で実装され、不正リテラルでコンパイルエラーになる
- `assert!` ベースの panic が全て削除されている
- `tests/test_limits.rs` で `unlimited` / `minimal` などの典型構成のテストが揃っている
- 既存の全テスト・PBT・fuzz が通る

## 関連

- [[0026-change-setting-construct-time-validation]] (`WindowSize` / `MaxFrameSize` を提供)
- [[0029-change-split-error-types]] (`LimitsError`)
