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
  (wire bytes ↔ HPACK encoded ↔ HeaderField) すべてで整合性を保つ必要がある

## 検証する不変性

### 1. 完全性 (`new` と decoder が同じ入力集合を受理)

```rust
// HeaderField
proptest! {
    #[test]
    fn new_accepts_iff_decoder_accepts(name in any_bytes(), value in any_bytes()) {
        let via_new = HeaderField::new(&name, &value).is_ok();
        let mut encoder = HpackEncoder::new();
        let wire = encoder.encode_literal(&name, &value);
        let mut decoder = HpackDecoder::new();
        let via_decoder = decoder.decode(&wire).is_ok();
        prop_assert_eq!(via_new, via_decoder, "new and decoder must agree on validity");
    }
}
```

### 2. 健全性 (ラウンドトリップ)

```rust
// HeaderField
proptest! {
    #[test]
    fn header_field_roundtrip(field in valid_header_field_strategy()) {
        let mut encoder = HpackEncoder::new();
        let wire = encoder.encode_field(&field);
        let mut decoder = HpackDecoder::new();
        let decoded = decoder.decode_one(&wire).unwrap();
        prop_assert_eq!(field, decoded);
    }
}

// Setting
proptest! {
    #[test]
    fn setting_roundtrip(setting in valid_setting_strategy()) {
        let (id, value) = setting.as_wire();
        let parsed = Setting::from_wire(id, value).unwrap();
        prop_assert_eq!(setting, parsed);
    }
}

// 各 Frame
proptest! {
    #[test]
    fn data_frame_roundtrip(frame in valid_data_frame_strategy()) {
        let mut encoder = FrameEncoder::new();
        encoder.encode(&Frame::Data(frame.clone())).unwrap();
        let wire = encoder.take_buffer();
        let mut decoder = FrameDecoder::new();
        decoder.feed(&wire);
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert!(matches!(decoded, Frame::Data(d) if d == frame));
    }
}
```

### 3. `from_static` と `new` の一貫性

`const fn` で書かれた `from_static` の検査ロジックと、ランタイム検査の `new` が
同じ判定をすることを担保する。リテラルではない値で両方を呼び比較する。

```rust
proptest! {
    #[test]
    fn header_field_static_matches_new(name in valid_name(), value in valid_value()) {
        // 注: from_static は &'static [u8] を要求するため、Box::leak で擬似的に静的化
        let name_static: &'static [u8] = Box::leak(name.clone().into_boxed_slice());
        let value_static: &'static [u8] = Box::leak(value.clone().into_boxed_slice());

        let via_new = HeaderField::new(&name, &value).unwrap();
        let via_static = HeaderField::from_static(name_static, value_static);
        prop_assert_eq!(via_new, via_static);
    }
}
```

### 4. `from_validated_parts` の整合性 (issue 0030 と統合)

```rust
proptest! {
    #[test]
    fn validated_parts_matches_new(name in valid_name(), value in valid_value()) {
        let via_new = HeaderField::new(&name, &value).unwrap();
        let via_validated = HeaderField::from_validated_parts(name, value);
        prop_assert_eq!(via_new, via_validated);
    }
}
```

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
        // CR/LF/NUL を除く field-vchar / OWS
    }

    pub fn valid_pseudo_header_name() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            Just(b":method".to_vec()),
            Just(b":scheme".to_vec()),
            // ...
        ]
    }

    pub fn valid_window_size() -> impl Strategy<Value = WindowSize> {
        (0u32..=WindowSize::MAX).prop_map(|s| WindowSize::new(s).unwrap())
    }

    // ... 各構築時検査型ごとに戦略を提供
}
```

## 影響範囲

- `pbt/src/lib.rs`: 戦略集約モジュール追加
- `pbt/tests/prop_header_field.rs` (新規): HeaderField の PBT
- `pbt/tests/prop_setting.rs` (新規): Setting の PBT
- `pbt/tests/prop_stream_id.rs` (新規): StreamId の PBT
- `pbt/tests/prop_frame.rs` (既存): 各フレームの構築時検査プロパティを追加
- `pbt/tests/prop_limits.rs` (新規): Limits の PBT

## CHANGES.md エントリ

```
- [ADD] 構築時検査の完全性 / 健全性 / `from_static` 一貫性 / `from_validated_parts` 整合性を
  検証する PBT を整備する
```

## 受け入れ条件

- `pbt/src/lib.rs` に各構築時検査型の `valid_*` / `invalid_*` 戦略が定義されている
- 「`new` と decoder が同じ入力集合を受理する」プロパティが全構築時検査型で実装されている
- ラウンドトリップ (encoder → decoder → 同値) プロパティが全構築時検査型で実装されている
- `from_static` と `new` の一貫性プロパティが実装されている
- `from_validated_parts` と `new` の整合性プロパティが実装されている
- 既存の全テスト・PBT・fuzz が通る

## 依存

- [[0024-change-header-field-construct-time-validation]]
- [[0025-change-stream-id-newtype]]
- [[0026-change-setting-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]]
- [[0028-change-limits-builder-result]]
- [[0030-add-from-validated-parts-internal-constructors]]
