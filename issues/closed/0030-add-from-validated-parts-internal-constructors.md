# 検証済み値用の pub(crate) コンストラクタを導入する

Created: 2026-05-23
Completed: 2026-05-24
Priority: Medium
Model: Opus 4.7

## 概要

構築時検査つきの公開 API (各 `Frame::new` 等) に対応して、decoder 内部で
「既に検証済みのバイト列から検査をスキップして構築する」`pub(crate) from_validated_parts(...)`
系を構築時検査型に導入する。

これにより、decoder が wire 上のバイト列を一度検査した結果を、公開コンストラクタで
再検査せずに構築できる。「検査責務はどこにあるか」をコード上で明示する。

注: `HeaderField::from_validated_parts` は issue 0024 で実装する。本 issue は
`HeaderField` 以外の構築時検査型 (`ClientStreamId`, `ServerStreamId`,
`NonZeroStreamId`, `WindowIncrement`, `Weight`, `LastStreamId`, `MaxFrameSize`,
`WindowSize`) の `from_validated_parts` を統一的に導入する。

`Setting` は issue 0026 で enum 化されるため、`from_validated_parts` の対象外とする。
enum 化後の `Setting` は `from_wire` が検証と構築を兼ねるため、二重検査の問題は発生しない。

## 背景

issue 0025 / 0027 で各構築点を `new() -> Result` に変更すると、decoder 経路で
二重検査が発生する可能性がある:

- decoder が wire 上の u32 をパースして `WindowIncrement::new(increment)` を呼ぶと、
  31 ビットマスク済みの値に対して再度範囲検査が走る
- decoder が wire 上の u32 を `ClientStreamId::new(id)` に渡すと、既に奇偶を確認済みの値に
  対して再度偶奇検査が走る

shiguredo_http11 では issue 0082 (`refactor-unify-from-validated-parts-cfg`) で同様の
内部コンストラクタを統一しており、設計パターンが確立している。

## 根拠

- 構築時検査を入れた直後は「二重検査でも正しい」が、「検査の単一責務」の観点で
  内部コンストラクタを分けるべき
- `pub(crate)` で外部 API には露出させないため、安全性は維持される
- decoder のテストで「検査をスキップしたパスでも同じ結果が得られる」を PBT で担保すれば、
  両経路の整合性を保証できる

## 設計

### 命名規則

全構築時検査型で `from_validated_parts` 系の命名を統一する。

```rust
impl ClientStreamId {
    pub fn new(id: u32) -> Result<Self, StreamIdError>;
    pub(crate) fn from_validated_parts(id: NonZeroU32) -> Self;
}

impl ServerStreamId {
    pub fn new(id: u32) -> Result<Self, StreamIdError>;
    pub(crate) fn from_validated_parts(id: NonZeroU32) -> Self;
}

impl NonZeroStreamId {
    pub fn new(id: u32) -> Result<Self, StreamIdError>;
    pub(crate) fn from_validated_parts(id: NonZeroU32) -> Self;
}

impl WindowIncrement {
    pub fn new(increment: u32) -> Result<Self, FrameError>;
    pub(crate) fn from_validated_parts(increment: NonZeroU32) -> Self;
}

impl Weight {
    pub fn new(weight: u16) -> Result<Self, FrameError>;
    pub(crate) fn from_validated_parts(weight: u16) -> Self;
}

impl LastStreamId {
    pub fn new(id: u32) -> Result<Self, FrameError>;
    pub(crate) fn from_validated_parts(id: u32) -> Self;
}
// ... 他の構築時検査型も同様
```

引数型を「不変条件を表現する型」(`NonZeroU32` 等) にすることで、`pub(crate)` 経路でも
完全な無検査ではなく型レベルで最低限の不変条件は強制する。

### debug_assert!

`from_validated_parts` には `debug_assert!` で不変条件を確認するコードを入れる。
リリースビルドではコストゼロ、debug ビルドで検査が破られていれば即 panic で検出できる。

例 (`WindowIncrement`):

```rust
pub(crate) fn from_validated_parts(increment: NonZeroU32) -> Self {
    debug_assert!(
        increment.get() <= (1u32 << 31) - 1,
        "window increment must be <= 2^31 - 1"
    );
    Self(increment)
}
```

各型で当該型の全不変条件を `debug_assert!` に含めること。

### PBT による整合性検証

`from_validated_parts` と公開 API (`new` 等) の結果が一致することを PBT で検証する。
`from_validated_parts` は `pub(crate)` のため、PBT は `src/` 内の `#[cfg(test)] mod tests`
として実装する (integration test crate からは呼べない)。

```rust
// src/frame/mod.rs 内の #[cfg(test)] mod tests
#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn validated_parts_matches_new(
            id in 1u32..=(1u32 << 31) - 1,
        ) {
            if id % 2 == 1 {
                let via_new = ClientStreamId::new(id).unwrap();
                let via_validated = ClientStreamId::from_validated_parts(
                    NonZeroU32::new(id).unwrap()
                );
                prop_assert_eq!(via_new, via_validated);
            }
        }
    }
}
```

## 影響範囲

- `src/frame/mod.rs`: `WindowIncrement`, `Weight`, `LastStreamId`, `NonZeroStreamId` に
  `from_validated_parts` を追加。`#[cfg(test)]` 内に整合性 PBT を追加
- `src/frame/decoder.rs`: `from_validated_parts` 経由に書き換え (フレーム型のみ)
- `src/stream/mod.rs` または `src/frame/mod.rs`: `ClientStreamId`, `ServerStreamId` に
  `from_validated_parts` を追加

注: `src/hpack/decoder.rs` の `HeaderField::from_validated_parts` への書き換えは
issue 0024 のスコープ内で実施する。

## CHANGES.md エントリ

```
- [UPDATE] decoder 内部で構築時検査型を組み立てる際に `pub(crate) from_validated_parts`
  を経由するようにし、二重検査を排除する
  - @担当者
```

## 受け入れ条件

- `HeaderField` 以外の全構築時検査型に `pub(crate) from_validated_parts` が実装されている
- `from_validated_parts` 内に `debug_assert!` で不変条件チェックが入っている
- フレーム decoder (`src/frame/decoder.rs`) が `from_validated_parts` 経由で構築している
- `from_validated_parts` と公開 API (`new` 等) の結果が一致することを `#[cfg(test)]` PBT で
  検証している
- 既存の全テスト・PBT・fuzz が通る

## 解決方法

- `WindowIncrement::from_validated_parts(NonZeroU32)` を追加し、decoder で非ゼロ検査済みの値から直接構築するようにした
- `Weight::from_validated_parts(u8)` を追加し、decoder で u8 から直接構築するようにした (u8 は常に 0..=255 に収まる)
- `LastStreamId::from_validated_parts(u32)` を追加し、decoder で 31-bit マスク済みの値から直接構築するようにした
- `NonZeroStreamId::from_validated_parts(NonZeroU32)` を追加し、decoder の `require_non_zero_stream_id` で使用するようにした
- `WindowSize::from_validated_parts(u32)` / `MaxFrameSize::from_validated_parts(u32)` を API 一貫性のために追加した (現時点では未使用)
- 全 `from_validated_parts` に `debug_assert!` で不変条件チェックを入れた
- `src/frame/error.rs`、`src/stream_id.rs`、`src/settings.rs` に `#[cfg(test)]` 内の PBT で `from_validated_parts` と `new` の整合性を検証した
- proptest を shiguredo_http2 の dev-dependencies に追加した

## 依存

- [[0024-change-header-field-construct-time-validation]] (`HeaderField::from_validated_parts` の先例)
- [[0025-change-stream-id-newtype]] (`ClientStreamId` / `ServerStreamId` / `NonZeroStreamId`)
- [[0027-change-frame-construct-time-validation]] (`WindowIncrement` / `Weight` / `LastStreamId`)
