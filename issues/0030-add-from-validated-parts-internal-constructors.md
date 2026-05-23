# 検証済み値用の pub(crate) コンストラクタを導入する

Created: 2026-05-23
Model: Opus 4.7

## 概要

構築時検査つきの公開 API (`HeaderField::new`, `Setting::from_wire`, 各 `Frame::new` 等) に
対応して、decoder 内部で「既に検証済みのバイト列から検査をスキップして構築する」
`pub(crate) from_validated_parts(...)` 系を全構築時検査型に導入する。

これにより、decoder が wire 上のバイト列を一度検査した結果を、公開コンストラクタで
再検査せずに構築できる。二重検査による性能劣化を防ぐと同時に、「検査責務はどこにあるか」を
コード上で明示する。

## 背景

issue 0024 / 0026 / 0027 で各構築点を `new() -> Result` に変更すると、decoder 経路で
以下の二重検査が発生する。

- decoder が wire 上の `[u8; N]` をパースして `Setting { id, value }` を組み立てる際、
  値範囲を確認済みなのに `Setting::from_wire(id, value)` で再検査
- decoder が HPACK で展開した name/value を `HeaderField::new(name, value)` に渡すと、
  HPACK 側で既に CRLF/NUL 検査をしているのに再度走る

shiguredo_http11 では issue 0082 (`refactor-unify-from-validated-parts-cfg`) で同様の
内部コンストラクタを統一しており、設計パターンが確立している。HTTP/2 でも最初から
統一して入れる。

## 根拠

- 構築時検査を入れた直後は「二重検査でも正しい」が、性能と「検査の単一責務」の観点で
  内部コンストラクタを分けるべき
- `pub(crate)` で外部 API には露出させないため、安全性は維持される
- decoder のテストで「検査をスキップしたパスでも同じ結果が得られる」を PBT で担保すれば、
  両経路の整合性を保証できる

## 設計

### 命名規則

全構築時検査型で `from_validated_parts` 系の命名を統一する。

```rust
impl HeaderField {
    pub fn new(name, value) -> Result<Self, HeaderFieldError>;
    pub(crate) fn from_validated_parts(name: Vec<u8>, value: Vec<u8>) -> Self;
}

impl Setting {
    pub fn from_wire(id: u16, value: u32) -> Result<Self, SettingError>;
    pub(crate) fn from_validated_parts(id: u16, value: u32) -> Self;
}

impl ClientStreamId {
    pub fn new(id: u32) -> Result<Self, StreamIdError>;
    pub(crate) fn from_validated_parts(id: NonZeroU32) -> Self;
}

impl WindowIncrement {
    pub fn new(increment: u32) -> Result<Self, FrameError>;
    pub(crate) fn from_validated_parts(increment: NonZeroU32) -> Self;
}
// ... 他の構築時検査型も同様
```

引数型を「不変条件を表現する型」(`NonZeroU32` 等) にすることで、`pub(crate)` 経路でも
完全な無検査ではなく型レベルで最低限の不変条件は強制する。

### debug_assert!

`from_validated_parts` には `debug_assert!` で不変条件を確認するコードを入れる。

```rust
pub(crate) fn from_validated_parts(name: Vec<u8>, value: Vec<u8>) -> Self {
    debug_assert!(!name.is_empty(), "field-name must not be empty");
    debug_assert!(
        name.iter().all(|b| b.is_ascii_lowercase() || !b.is_ascii_alphabetic()),
        "field-name must be lowercase"
    );
    debug_assert!(
        !value.iter().any(|&b| b == 0x00 || b == 0x0D || b == 0x0A),
        "field-value must not contain NUL/CR/LF"
    );
    Self { name, value }
}
```

リリースビルドではコストゼロ、debug ビルドで検査が破られていれば即 panic で検出できる。

### PBT による整合性検証

各構築時検査型に対して、以下のプロパティを PBT で検証する。

```rust
// 例: HeaderField
proptest! {
    #[test]
    fn validated_parts_matches_new(name in valid_name_strategy(), value in valid_value_strategy()) {
        let via_new = HeaderField::new(&name, &value).unwrap();
        let via_validated = HeaderField::from_validated_parts(name, value);
        prop_assert_eq!(via_new, via_validated);
    }
}
```

`from_validated_parts` を使う decoder と、`new` を使う公開 API が同じ結果を返すことを
保証する。

## 影響範囲

- 各構築時検査型のファイルに `from_validated_parts` を追加
- `src/frame/decoder.rs`: `from_validated_parts` 経由に書き換え
- `src/hpack/decoder.rs`: `from_validated_parts` 経由に書き換え
- `pbt/tests/`: 整合性プロパティを追加

## CHANGES.md エントリ

```
- [UPDATE] decoder 内部で構築時検査型を組み立てる際に `pub(crate) from_validated_parts`
  を経由するようにし、二重検査を排除する
```

## 受け入れ条件

- 全構築時検査型に `pub(crate) from_validated_parts` が実装されている
- `from_validated_parts` 内に `debug_assert!` で不変条件チェックが入っている
- 各 decoder が `from_validated_parts` 経由で構築している
- `from_validated_parts` と公開 API (`new` 等) の結果が一致することを PBT で検証している
- 既存の全テスト・PBT・fuzz が通る

## 依存

- [[0024-change-header-field-construct-time-validation]]
- [[0025-change-stream-id-newtype]]
- [[0026-change-setting-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]]
