# cargo feature `__test_helpers` を廃止し PBT / fuzz を公開 API + wire 経路に移行する

- Priority: Medium
- Created: 2026-05-24
- Model: Composer 2.5
- Branch: feature/refactor-remove-test-helpers-feature

## 目的

issue 0024 / 0033 で導入した cargo feature `__test_helpers` は、PBT / fuzz クレートから `HeaderField::from_validated_parts` および crate 内部の const fn / runtime 検査関数を呼ぶために **テスト専用の公開 API 表面** を増やしている。本番利用禁止 feature は運用負担が大きく、issue 0036 の方針「テストのために API 表面を広げない」とも矛盾する。

http11-rs では同等の feature を使わず、**公開 decoder に任意バイト列を食わせる** fuzz と、**公開 API のみ**の PBT で検証している。http2-rs も同型に揃え、型不変条件を破壊しうる内部 API の crate 外露出をやめる。

## 優先度根拠

Medium。機能欠落ではないが、0024 以降のテスト基盤の設計負債であり、0037 (fuzz CI) や 0039 (PBT 命名) など後続 issue の記述にも `__test_helpers` 依存が残っている。早めに解消しないと「feature 必須のテスト実行」が恒久化する。

## 制約 (本 issue で譲らないこと)

- **PBT は必ず `pbt/` 以下に置く**。`src/<module>` 内 `#[cfg(test)] mod` への PBT 移管は採用しない (CLAUDE.md テスト規約との整合性を保つため)。
- **ライブラリ crate の API 表面をテスト目的で広げない** (`pub` 昇格、新規 cargo feature、`#[doc(hidden)] pub` モジュール追加はすべて不採用)。

## 現状

| 箇所 | `__test_helpers` 依存内容 |
|---|---|
| `Cargo.toml` | `[features] __test_helpers = []` |
| `src/__test_helpers.rs` | panic-catch ラッパ 6 関数 + `header_field_from_validated_parts` |
| `src/lib.rs` | `#[cfg(feature = "__test_helpers")] pub mod __test_helpers` |
| `pbt/Cargo.toml` | `features = ["__test_helpers"]` |
| `pbt/tests/prop_header_field_syntax.rs` | 内部 `check_*_const` / `validate_*` の同値性 PBT |
| `pbt/tests/prop_validation.rs` | wire 模擬の `header_field_from_validated_parts` (7 プロパティ) |
| `fuzz/Cargo.toml` | `features = ["__test_helpers"]` |
| `fuzz/fuzz_targets/fuzz_validation.rs` | 任意バイト列 → `from_validated_parts` → `validate_*` |
| `fuzz/fuzz_targets/fuzz_hpack_roundtrip.rs` | 任意バイト列 → `from_validated_parts` → encode/decode |

HPACK decoder (`HpackDecoder::decode`) は内部で `HeaderField::from_validated_parts` を呼ぶため、**wire 上の任意 name/value は decoder 経路で再現可能** (本番と同じ経路)。

## 設計方針

### 1. `prop_header_field_syntax.rs` — 公開 API 同値性 PBT に書き換え

内部関数の直接比較をやめ、以下の公開 API 同士で accept/reject を比較する:

- `HeaderField::new(name, value)` — runtime 検査 (`validate_*`)
- `HeaderField::from_static(name, value)` — const 検査 (`check_*_const`)

`from_static` は `&'static [u8]` を要求するため、proptest 生成値は `Box::leak` で擬似静的化する (http11-rs issue 0093 と同型。2048 ケース程度の leak は許容)。

```rust
let via_new = HeaderField::new(&name, &value);
let leaked_name: &'static [u8] = Box::leak(name.clone().into_boxed_slice());
let leaked_value: &'static [u8] = Box::leak(value.clone().into_boxed_slice());
let via_static = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    HeaderField::from_static(leaked_name, leaked_value)
}));
prop_assert_eq!(via_new.is_ok(), via_static.is_ok(), ...);
```

既存の `name_strategy` / `value_strategy` は流用可能。3 プロパティ (`field-name` / `field-value` / `pseudo-header` 個別) は 1 プロパティ (`new` vs `from_static` 全体) に統合してよい (`from_static` doc: 「検査内容は `new` と等価」)。

### 2. `prop_validation.rs` — HPACK wire 経路で wire 模擬

`header_field_from_validated_parts` の代わりに **`pbt/src/lib.rs` に wire エンコードヘルパ** を追加し、公開 `HpackDecoder` 経由で `HeaderField` を得る:

```rust
/// 検査なしで name/value を Literal Header Field without Indexing として符号化し、
/// HpackDecoder でデコードして HeaderField を返す (wire 模擬)。
pub fn decode_wire_header_field(name: &[u8], value: &[u8]) -> HeaderField { ... }
```

符号化仕様 (RFC 7541 §6.2.2):

- 先頭バイト: `0x00` (Literal without Indexing, name index = 0)
- name: HPACK string (Huffman off, 長さプレフィックス + 生バイト)
- value: 同上

Huffman は off に固定 (実装単純化。検査対象は field syntax / validation であり圧縮形式は無関係)。

`prop_validation.rs` 内の 7 プロパティ (`prop_uppercase_header_name_rejected` 等) は `header_field_from_validated_parts(...)` 呼び出しを `decode_wire_header_field(...)` に置換する。

### 3. fuzz — decoder 経路へ移行 (http11 同型)

| fuzz target | 移行後 |
|---|---|
| `fuzz_validation.rs` | 任意 HPACK バイト列 (または wire ヘルパで符号化したブロック) → `HpackDecoder::decode` → 成功時 `validate_*` (panic しないこと) |
| `fuzz_hpack_roundtrip.rs` | 任意 HPACK バイト列 → decode → 成功時 encode → 再 decode → name/value 一致 (失敗は許容) |

`from_validated_parts` の直接呼び出しは削除。

### 4. 削除対象

- `src/__test_helpers.rs` ファイルごと削除
- `Cargo.toml` の `[features] __test_helpers`
- `src/lib.rs` の feature-gated モジュール宣言
- `src/hpack/table.rs` doc 内の `__test_helpers::header_field_from_validated_parts` 言及 (wire 模擬は decoder 経路と明記)
- `pbt/Cargo.toml` / `fuzz/Cargo.toml` の `features = ["__test_helpers"]`

### 5. 関連 issue 記述の更新 (本 issue 実装時)

- `issues/0037-add-fuzz-build-check-to-ci.md` の「`__test_helpers` feature の自動 ON」節を削除または書き換え (feature 廃止後は通常依存のみ)
- `issues/0036-refactor-move-mod-tests-to-tests-dir.md` L46 の「例外として `__test_helpers`...」記述を削除 (例外が消えるため)

### 6. CHANGES.md

`## develop` の `### misc` に `[CHANGE]` を追記:

- cargo feature `__test_helpers` を廃止し、PBT / fuzz は公開 API と HPACK decoder 経路のみで wire 模擬する

## 完了条件

- [ ] `__test_helpers` feature / モジュール / 依存指定がコードベースから完全に消えている
- [ ] `pbt/src/lib.rs` に `decode_wire_header_field` (または同等名) が実装されている
- [ ] `pbt/tests/prop_header_field_syntax.rs` が `HeaderField::new` vs `from_static` 同値性 PBT に書き換わっている
- [ ] `pbt/tests/prop_validation.rs` の wire 模擬 7 件が decoder 経路に置き換わっている
- [ ] `fuzz/fuzz_targets/fuzz_validation.rs` / `fuzz_hpack_roundtrip.rs` が feature なしでコンパイル・実行可能
- [ ] `cargo test --workspace` が `--features __test_helpers` なしで通る (PBT 含む)
- [ ] `cargo check --manifest-path fuzz/Cargo.toml` が通る
- [ ] `cargo clippy --workspace -- -D warnings` が通る
- [ ] `CHANGES.md` に `[CHANGE]` エントリが追加されている

## 解決方法 (実装手順)

1. `pbt/src/lib.rs` に HPACK literal wire 符号化 + `HpackDecoder` デコードヘルパを実装
2. `pbt/tests/prop_header_field_syntax.rs` を公開 API 同値性 PBT に書き換え
3. `pbt/tests/prop_validation.rs` の `header_field_from_validated_parts` を wire ヘルパに置換
4. `fuzz/fuzz_targets/fuzz_validation.rs` / `fuzz_hpack_roundtrip.rs` を decoder 経路に書き換え
5. `src/__test_helpers.rs` 削除、`Cargo.toml` / `src/lib.rs` / `pbt/Cargo.toml` / `fuzz/Cargo.toml` から feature 関連を削除
6. `src/hpack/table.rs` doc 更新
7. 0036 / 0037 issue 記述更新
8. `CHANGES.md` 追記
9. 上記完了条件のコマンドをすべて実行して確認

## 関連

- [[0024-change-header-field-construct-time-validation]] (`__test_helpers` 導入元)
- [[0033-refactor-dedupe-from-validated-parts-cfg]] (公開層を `__test_helpers` に集約)
- [[0036-refactor-move-mod-tests-to-tests-dir]] (例外条項が本 issue で削除対象)
- [[0037-add-fuzz-build-check-to-ci]] (`__test_helpers` 記述が本 issue で更新対象)
- http11-rs: fuzz は feature なし decoder 経路、PBT は公開 API のみ (参考実装)
