# 構築時検査の PBT を整備する

Created: 2026-05-23
Model: Opus 4.7

## 概要

issue 0024 / 0025 / 0026 / 0027 / 0028 で導入する構築時検査について、PBT で以下を担保する。

1. **完全性**: `new() -> Result` が `Ok` を返す入力集合と、decoder が `Ok` を返す入力集合が一致
2. **健全性**: `new()` が `Ok` を返した値は encoder → decoder の往復で同値を返す
3. **コンパイル時検査の一貫性**: `const fn from_static` が成功するリテラルは `new()` でも成功する

これらが揃って初めて「構築時検査と decoder の検査が同じ RFC ルールを実装している」と
言える状態になる。

## 背景

構築時検査と decoder 側の検査を別々に実装すると、以下の不整合が起きやすい。

- 構築 API は受け入れるが decoder が拒否する値 → ネットワーク越しの相互運用で送信できない
- 構築 API は拒否するが decoder が受け入れる値 → リモートから受信した値を中継しようとして失敗
- `from_static` (`const fn`) と `new` の検査ロジックが微妙にズレる → ローカルでは通るが
  本番で違反値を許してしまう

これらを PBT で恒常的に検証する仕組みを入れる。

## 根拠

- 構築時検査は「ライブラリの中で複数経路に分散する検査ロジック」(public API, const fn,
  decoder, from_validated_parts debug_assert) の整合性が壊れやすい
- shiguredo_http11 は decoder/encoder のラウンドトリップ PBT (`pbt/tests/prop_decoder/`,
  `prop_encoder.rs`) で同様の不変性を担保している
- HTTP/2 は HPACK 経由でヘッダーが圧縮されるため、ラウンドトリップの中間表現
  (wire bytes → HPACK encoded → HeaderField) すべてで整合性を保つ必要がある

## 検証する不変性

注: 以下のコードは検証対象のプロパティを示す **疑似コード** であり、実際の API 名とは
異なる場合がある。実装時は実際の `Encoder::encode` / `Decoder::decode` 等のシグネチャに
合わせること。

### 1. 完全性 (`new` と decoder が同じ入力集合を受理)

```rust
// 疑似コード: HeaderField
// 実際の Encoder::encode は &mut Vec<u8> と &[HeaderField] を取る
// 実際の Decoder::decode は &[u8] を取り Result<Vec<HeaderField>> を返す
proptest! {
    #[test]
    fn new_accepts_iff_decoder_accepts(name in any_bytes(), value in any_bytes()) {
        let via_new = HeaderField::new(&name, &value).is_ok();
        // encoder/decoder を使ってワイヤ表現経由で検査
        // (具体的な呼び出し方は実装時に Encoder/Decoder API に合わせる)
        // via_new と via_decoder が一致することを検証
    }
}
```

### 2. 健全性 (ラウンドトリップ)

```rust
// 疑似コード: Setting
proptest! {
    #[test]
    fn setting_roundtrip(setting in valid_setting_strategy()) {
        let (id, value) = setting.as_wire();
        let parsed = Setting::from_wire(id, value).unwrap();
        prop_assert_eq!(setting, parsed);
    }
}

// 疑似コード: 各 Frame (実装時は FrameEncoder/FrameDecoder の実 API に合わせる)
proptest! {
    #[test]
    fn data_frame_roundtrip(frame in valid_data_frame_strategy()) {
        // FrameEncoder::encode で wire bytes に変換
        // FrameDecoder::new(max_frame_size).decode() で復元
        // 同値であることを検証
    }
}
```

### 3. `from_static` と `new` の一貫性

`const fn` で書かれた `from_static` の検査ロジックと、ランタイム検査の `new` が
同じ判定をすることを担保する。

```rust
// 疑似コード
proptest! {
    #[test]
    fn window_size_static_matches_new(size in 0u32..=WindowSize::MAX) {
        let via_new = WindowSize::new(size).unwrap();
        let via_static = WindowSize::from_static(size);
        prop_assert_eq!(via_new, via_static);
    }
}
```

注: HeaderField の `from_static` テストは `&'static [u8]` を要求するため `Box::leak` で
擬似的に静的化する必要がある。PBT は数千ケース実行されるため、メモリリークに注意が必要。
テストケース数を制限するか、`from_static` の一貫性テストは `from_static` と `new` の
検査ロジックが共通関数を呼ぶことをコードレビューで確認する運用に代替することも検討する。

### 4. `from_validated_parts` の整合性 (issue 0030 と統合)

`from_validated_parts` は `pub(crate)` のため、この検証は `src/` 内の `#[cfg(test)] mod tests`
として実装する (integration test crate の `pbt/tests/` からは呼べない)。
詳細は issue 0030 を参照。

## 戦略 (Strategy) 設計

各構築時検査型ごとに、`valid_*_strategy()` と `invalid_*_strategy()` を `pbt/src/lib.rs` に
集約する。

```rust
// pbt/src/lib.rs
pub mod strategies {
    use proptest::prelude::*;
    use shiguredo_http2::*;

    pub fn valid_field_name() -> impl Strategy<Value = Vec<u8>> {
        // RFC 9113 §8.2.1 準拠: lowercase + token-char
    }

    pub fn valid_field_value() -> impl Strategy<Value = Vec<u8>> {
        // CR/LF/NUL を除く field-vchar、先頭/末尾に SP/HTAB なし
    }

    pub fn valid_pseudo_header_name() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            Just(b":method".to_vec()),
            Just(b":scheme".to_vec()),
            Just(b":authority".to_vec()),
            Just(b":path".to_vec()),
            Just(b":status".to_vec()),
            Just(b":protocol".to_vec()),
        ]
    }

    pub fn valid_window_size() -> impl Strategy<Value = WindowSize> {
        (0u32..=WindowSize::MAX).prop_map(|s| WindowSize::new(s).unwrap())
    }

    pub fn valid_max_frame_size() -> impl Strategy<Value = MaxFrameSize> {
        (MaxFrameSize::MIN..=MaxFrameSize::MAX).prop_map(|s| MaxFrameSize::new(s).unwrap())
    }

    // ... 各構築時検査型ごとに戦略を提供
}
```

## 影響範囲

- `pbt/src/lib.rs`: 戦略集約モジュール (`strategies`) を追加
- `pbt/tests/prop_hpack.rs` (既存に追記): HeaderField の完全性・健全性・from_static 一貫性 PBT
- `pbt/tests/prop_settings.rs` (既存に追記): Setting のラウンドトリップ・from_static 一貫性 PBT
- `pbt/tests/prop_frame.rs` (既存に追記): 各フレーム型・StreamId 型の PBT
- `pbt/tests/prop_limits.rs` (新規): Limits の `build()` 複合制約検査の PBT
  (有効な `LimitsBuilder` 設定のラウンドトリップ、無効な設定の `Err` 検証)

## CHANGES.md エントリ

```
- [ADD] 構築時検査の完全性・健全性・from_static 一貫性・from_validated_parts 整合性を検証する PBT を整備する
  - @担当者
```

## 受け入れ条件

- `pbt/src/lib.rs` に各構築時検査型の `valid_*` / `invalid_*` 戦略が定義されている
- 「`new` と decoder が同じ入力集合を受理する」プロパティが全構築時検査型で実装されている
- ラウンドトリップ (encoder → decoder → 同値) プロパティが全構築時検査型で実装されている
- `from_static` と `new` の一貫性プロパティが実装されている
- `from_validated_parts` と `new` の整合性プロパティが `#[cfg(test)]` 内で実装されている
- 既存の全テスト・PBT・fuzz が通る

## 依存

- [[0024-change-header-field-construct-time-validation]]
- [[0025-change-stream-id-newtype]]
- [[0026-change-setting-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]]
- [[0028-change-limits-builder-result]]
- [[0030-add-from-validated-parts-internal-constructors]]
