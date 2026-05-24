# cargo feature `__test_helpers` を廃止し PBT / fuzz を公開 API + wire 経路に移行する

- Priority: Medium
- Created: 2026-05-24
- Completed: 2026-05-24
- Model: Composer 2.5
- Branch: feature/refactor-remove-test-helpers-feature

## 目的

issue 0024 / 0033 で導入した cargo feature `__test_helpers` は、PBT / fuzz クレートから `HeaderField::from_validated_parts` および crate 内部の const fn / runtime 検査関数を呼ぶためにテスト専用の公開 API 表面を増やしている。本番利用禁止 feature は Cargo の feature unification で意図せず有効化されるリスクがあり、issue 0036 の方針「テストのために API 表面を広げない」とも矛盾する。

本 issue では `__test_helpers` feature を廃止し、PBT / fuzz を公開 API と HPACK decoder 経路のみで検証する構成に移行する。

## 優先度根拠

Medium。機能欠落ではないが、0024 以降のテスト基盤の設計負債であり、0037 (fuzz CI) や 0039 (PBT 命名) の完了条件にも `--features __test_helpers` が残っている。早めに解消しないと「feature 必須のテスト実行」が恒久化する。

## 現状

| 箇所 | `__test_helpers` 依存内容 |
|---|---|
| `Cargo.toml` | `[features] __test_helpers = []` |
| `src/__test_helpers.rs` | panic-catch ラッパ 3 関数 (`check_*_const_result`) + runtime 検査ラッパ 3 関数 (`validate_*_result`) + `header_field_from_validated_parts` |
| `src/lib.rs` | `#[cfg(feature = "__test_helpers")] pub mod __test_helpers` |
| `pbt/Cargo.toml` | `features = ["__test_helpers"]` |
| `pbt/tests/prop_header_field_syntax.rs` | 内部 `check_*_const` / `validate_*` の同値性 PBT (3 プロパティ) |
| `pbt/tests/prop_validation.rs` | `header_field_from_validated_parts` 使用 (7 箇所・7 プロパティ) |
| `fuzz/Cargo.toml` | `features = ["__test_helpers"]` |
| `fuzz/fuzz_targets/fuzz_validation.rs` | 任意バイト列 → `from_validated_parts` → `validate_*` |
| `fuzz/fuzz_targets/fuzz_hpack_roundtrip.rs` | 任意バイト列 → `from_validated_parts` → encode/decode |

HPACK decoder (`HpackDecoder::decode`) は内部で `HeaderField::from_validated_parts` を呼ぶため、HPACK Literal Header Field without Indexing (RFC 7541 §6.2.2) として符号化すれば、wire 上の任意 name/value を公開 decoder 経路で再現可能 (本番と同じ経路)。

## 設計方針

### 原則

- PBT は `pbt/` 以下に置く (CLAUDE.md テスト規約)。ただし `pub(crate)` 関数を直接叩く PBT は crate 内 `#[cfg(test)] mod tests` に残す (0036 設計方針 2)
- ライブラリ crate の API 表面をテスト目的で広げない (`pub` 昇格、新規 cargo feature、`#[doc(hidden)] pub` 追加はすべて不採用)

### 1. `prop_header_field_syntax.rs` — crate 内 `#[cfg(test)]` に移管

現在の `pbt/tests/prop_header_field_syntax.rs` は `check_field_name_const` (const fn 検査、`pub(crate)`) と `validate_field_name` (runtime 検査、`pub(crate)`) の同値性を検証する PBT である。両関数とも `pub(crate)` であり、`__test_helpers` を廃止すると crate 外から呼べなくなる。公開 API (`HeaderField::new` vs `from_static`) での代替は以下の理由で不採用:

- `from_static` は `const fn` であり、設計意図は「不正リテラルをコンパイル時に検出すること」(table.rs L71-L89)。runtime で `catch_unwind` 越しに呼ぶのは設計意図と異なる経路をテストすることになる
- `from_static` が `&'static [u8]` を要求するため `Box::leak` による擬似静的化が必要になり、shrinking 時のリーク増大リスクがある
- 3 プロパティ (`field-name` / `field-value` / `pseudo-header`) を 1 プロパティに統合すると、乖離箇所の特定粒度が低下する。特に `field-value` 検査は単独テストで name を固定値にしてカバレッジを確保しているが、統合すると name が不正な場合に value 検査に到達しなくなる

**移管先**: `src/hpack/table.rs` の `#[cfg(test)] mod tests`。ルートクレートは既に `proptest` を `[dev-dependencies]` に持つため、依存追加なしで `proptest!` マクロを使用できる。`#[cfg(test)]` 内から `crate::hpack::bytes::check_field_name_const` 等を直接呼べる。

既存の 3 プロパティ (`prop_check_field_name_equivalence`, `prop_check_field_value_equivalence`, `prop_check_pseudo_header_equivalence`) と strategy (`name_strategy`, `value_strategy`) を移管し、3 プロパティの個別検証を維持する。

**panic-catch ヘルパの移管時の注意**: 現行の `install_silent_panic_hook` は `std::panic::set_hook` でグローバル panic hook を無音化し、`Once` で二度と復元しない。pbt の integration test では独立プロセスのため問題なかったが、`#[cfg(test)] mod tests` に移管すると `cargo test --lib` で同一プロセス内の他テストの panic 出力も抑制される。移管時は `Once` + `set_hook` 方式をやめ、各 `catch_unwind` 呼び出しの前に `std::panic::take_hook()` で退避し、`catch_unwind` 後に復元するスコープ限定方式に変更する。

**テストモジュール構成**: table.rs の `mod tests` は既に 27 テスト (約 224 行) がある。proptest 3 プロパティ + strategy + panic-catch ヘルパ (約 70 行) を追加すると約 300 行になる。`mod tests` 内に `mod syntax_equivalence` サブモジュールを作り、移管した proptest コードを分離する。

**proptest バージョン**: ルートクレートの `[dev-dependencies]` は `proptest = "1.6"`、pbt は `proptest = "1.11"`。SemVer 上 `"1.6"` は `>=1.6.0, <2.0.0` を指すため `1.11.x` も解決可能。ただしバージョン統一のため、ルートクレートのバージョン指定を `"1.11"` に更新する。

移管後 `pbt/tests/prop_header_field_syntax.rs` は削除する。

### 2. `prop_validation.rs` — HPACK wire 経路で wire 模擬

`header_field_from_validated_parts` の代わりに、`pbt/src/lib.rs` に HPACK wire 符号化 + `HpackDecoder` デコードのヘルパ関数を追加し、公開 API のみで `HeaderField` を構築する。

```rust
/// 検査なしの name/value を HPACK Literal Header Field without Indexing
/// (RFC 7541 §6.2.2) として符号化し、HpackDecoder でデコードして
/// HeaderField を返す (wire 模擬)。
pub fn wire_header_field(name: &[u8], value: &[u8]) -> HeaderField { ... }
```

符号化仕様 (RFC 7541 §6.2.2, §5.2, §5.1):

- 先頭バイト: `0x00` (4-bit `0000` pattern + name index = 0)
- name: HPACK string literal (H=0, String Length を 7-bit prefix 整数 (§5.1) で符号化、その後 raw octets)
- value: 同上

String Length の符号化には公開 API `shiguredo_http2::hpack::integer::encode` を使用する。128 バイト以上の name/value でも可変長整数表現で正しく符号化される。Huffman は off に固定する (検査対象は field syntax / validation であり圧縮形式は無関係)。

`HpackDecoder` の動的テーブルサイズは 0 に設定する (Literal without Indexing のため動的テーブルへの追加は発生しない)。

**`pbt/Cargo.toml` の依存変更**: `shiguredo_http2` を `[dev-dependencies]` から `[dependencies]` に移動し、`features = ["__test_helpers"]` を削除する。`pbt/src/lib.rs` のライブラリコードから `shiguredo_http2` の型 (`HpackDecoder`, `HeaderField`, `hpack::integer::encode`) を使用するため必須。`pbt` は `publish = false` のため、利用者への影響はない。

`prop_validation.rs` 内の 7 プロパティは `header_field_from_validated_parts(...)` 呼び出しを `pbt::wire_header_field(...)` に置換する:

- `prop_uppercase_header_name_rejected` (L227)
- `prop_header_name_with_invalid_chars_rejected` (L503)
- `prop_header_value_with_nul_rejected` (L524)
- `prop_header_value_with_cr_lf_rejected` (L546)
- `prop_header_value_leading_whitespace_rejected` (L588)
- `prop_header_value_trailing_whitespace_rejected` (L608)
- `prop_header_value_internal_space_accepted` (L627: 正常データ受理の確認)

### 3. fuzz — wire ヘルパ方式に移行

HPACK decoder に任意バイト列を直接渡す方式ではなく、**wire ヘルパで任意 name/value を HPACK literal に符号化してから decoder に通す方式** を採用する。理由:

- 任意 HPACK バイト列を decoder に食わせる方式は `fuzz_hpack_decoder` (既存、変更不要) と役割が重複する
- decoder が不正な HPACK 構造 (不完全な整数 prefix、不正な Huffman 符号等) を拒否するため、validation 層や encoder/decoder roundtrip に到達するケースが激減し、カバレッジが大幅に後退する
- wire ヘルパ方式なら任意の name/value を `HeaderField` に格納でき、現行の `from_validated_parts` と同等のカバレッジを維持できる。HPACK encode/decode を経由するオーバーヘッドがあるが、fuzz の目的はパニック安全性の検証であり速度は二次的

fuzz クレートから pbt クレートへの path 依存追加は技術的に可能だが、fuzz target の自己完結性を優先し、各 fuzz target 内に wire 符号化ヘルパを定義する。`shiguredo_http2::hpack::integer::encode` (公開 API) を使用する。wire ヘルパのロジック修正時は pbt/fuzz の 3 箇所を同期更新する必要がある。

wire ヘルパは任意の name/value に対して HPACK エラーを起こさない。encode 側で正しい HPACK literal を構築するため、decode は必ず成功する。`expect` は実装バグ検出用。

```rust
fn wire_header_field(name: &[u8], value: &[u8]) -> shiguredo_http2::HeaderField {
    let mut wire = Vec::new();
    wire.push(0x00);
    encode_string(&mut wire, name);
    encode_string(&mut wire, value);
    let mut decoder = shiguredo_http2::HpackDecoder::new(0);
    let headers = decoder.decode(&wire)
        .expect("infallible: wire_header_field produced invalid HPACK");
    headers.into_iter().next()
        .expect("infallible: wire encoding produces exactly one header")
}

/// HPACK string literal (RFC 7541 §5.2) を符号化する。
/// H=0 (Huffman off)、String Length は 7-bit prefix 整数 (§5.1) で符号化。
/// 16 バイトバッファは 7-bit prefix 整数の最大長 (u64 で 11 バイト) に十分。
fn encode_string(buf: &mut Vec<u8>, data: &[u8]) {
    let mut temp = [0u8; 16];
    let len = shiguredo_http2::hpack::integer::encode(
        &mut temp, data.len() as u64, 7, 0x00,
    ).expect("infallible: 16 bytes exceeds HPACK integer maximum of 11 bytes");
    buf.extend_from_slice(&temp[..len]);
    buf.extend_from_slice(data);
}
```

| fuzz target | 移行後 |
|---|---|
| `fuzz_validation.rs` | `FuzzInput { headers: Vec<FuzzHeader> }` 構造を維持 → wire ヘルパで各 header を `HeaderField` に変換 → `validate_*` (panic しないこと) |
| `fuzz_hpack_roundtrip.rs` | `FuzzInput` 構造を維持 → wire ヘルパで各 header を `HeaderField` に変換 → encode → decode → name/value 一致 (注: wire ヘルパ内で decode → 再度 encode → decode の 3 段構成になるが、最初の decode は HeaderField 構築のための手段であり、テスト対象は 2 段目の encode → decode roundtrip) |

`fuzz_hpack_decoder` (任意バイト列 → decode) との差別化: `fuzz_hpack_decoder` は HPACK 構造不正に対する decoder のパニック安全性を検証する。上記 2 target は name/value 内容の任意性に対する後段 (validation / roundtrip) のパニック安全性を検証する。

注: 現行・移行後とも `sensitive` フラグは `false` 固定 (Literal without Indexing, `0x00` prefix)。将来 `sensitive: true` (Never Indexed, `0x10` prefix) の wire 模擬が必要になった場合は wire ヘルパの prefix を切り替える引数を追加する。

### 4. 削除対象

- `src/__test_helpers.rs` ファイルごと削除
- `Cargo.toml` の `[features] __test_helpers`
- `src/lib.rs` の feature-gated モジュール宣言
- `src/hpack/table.rs` L107-L108 の doc 更新: `__test_helpers::header_field_from_validated_parts` ラッパの言及を削除し、crate 外からの wire 模擬は `HpackDecoder` 経路を使用する旨に変更
- `pbt/tests/prop_header_field_syntax.rs` (crate 内 `#[cfg(test)]` に移管済み)
- `pbt/Cargo.toml` の `features = ["__test_helpers"]` (依存は `[dependencies]` に移動、feature 指定なし)
- `fuzz/Cargo.toml` の `features = ["__test_helpers"]`

### 5. 関連 issue 記述の更新

- `issues/0036-refactor-move-mod-tests-to-tests-dir.md`: L46 の「例外として `__test_helpers`...」記述を削除、L92-L93 の `--features __test_helpers` 確認条件を削除、L121 の CI 確認記述から `--features __test_helpers` を削除
- `issues/0037-add-fuzz-build-check-to-ci.md`: L45 の `__test_helpers` feature 経由再 check の記述を削除、L47-L49 の「`__test_helpers` feature の自動 ON」節を削除
- `issues/0039-fix-pbt-naming-convention.md`: L98-L100 の `__test_helpers.rs` の扱い節を削除、L115/L117 の `--features __test_helpers` 完了条件を更新、L140 の `src/__test_helpers.rs` スコープ外記述を削除

### 6. CHANGES.md

`## develop` の `### misc` に追記:

```
- [UPDATE] cargo feature `__test_helpers` を廃止し、PBT / fuzz は公開 API と HPACK decoder 経路のみで wire 模擬する
  - @voluntas
```

種別 `[UPDATE]` の根拠: `__test_helpers` は `#[doc(hidden)]` かつ doc で「本番利用禁止」と明記された内部テスト用 feature であり、crate の公開 API 表面には含まれない。`publish = false` の pbt/fuzz のみが使用しており、crate 利用者のビルドに影響しない。

## 完了条件

- [ ] `__test_helpers` feature / モジュール / 依存指定がコードベースから完全に消えている (`grep -rn __test_helpers` で 0 件)
- [ ] `pbt/src/lib.rs` に `wire_header_field` が実装されている
- [ ] `pbt/Cargo.toml` で `shiguredo_http2` が `[dependencies]` に feature 指定なしで記載されている
- [ ] `src/hpack/table.rs` の `#[cfg(test)] mod tests` に const/runtime 同値性 proptest (3 プロパティ) が移管されている
- [ ] `pbt/tests/prop_header_field_syntax.rs` が削除されている
- [ ] `pbt/tests/prop_validation.rs` の wire 模擬 7 箇所が `pbt::wire_header_field` に置き換わっている
- [ ] `fuzz/fuzz_targets/fuzz_validation.rs` / `fuzz_hpack_roundtrip.rs` が feature なし・wire ヘルパ方式でコンパイル・実行可能
- [ ] `cargo test --workspace` が `--features __test_helpers` なしで通る (PBT 含む)
- [ ] `cargo check --manifest-path fuzz/Cargo.toml` が通る
- [ ] `cargo clippy --workspace -- -D warnings` が通る
- [ ] `cargo clippy --manifest-path fuzz/Cargo.toml -- -D warnings` が通る
- [ ] 移行前後で `cargo llvm-cov report` の `validation.rs` / `table.rs` の行カバレッジが同等以上
- [ ] 0036 / 0037 / 0039 の `__test_helpers` 関連記述が更新されている
- [ ] `CHANGES.md` の `### misc` に `[UPDATE]` エントリが追加されている

## 解決方法

コミット `07fed49` (PR #12) で対応済み。

### 削除したもの

- `src/__test_helpers.rs` (panic-catch ラッパ 3 関数、runtime 検査ラッパ 3 関数、`header_field_from_validated_parts`)
- `Cargo.toml` の `[features] __test_helpers = []`
- `src/lib.rs` の `#[cfg(feature = "__test_helpers")] pub mod __test_helpers`
- `pbt/tests/prop_header_field_syntax.rs`
- `pbt/Cargo.toml` / `fuzz/Cargo.toml` の `features = ["__test_helpers"]`

### 移管・追加したもの

- `src/hpack/table.rs` の `#[cfg(test)] mod tests` 内に `mod syntax_equivalence` を追加し、const/runtime 同値性 proptest 3 プロパティと strategy、スコープ限定 panic-catch ヘルパを移管
- `pbt/src/lib.rs` に `wire_header_field` ヘルパを追加 (HPACK Literal Header Field without Indexing で符号化し `HpackDecoder` でデコードする wire 模擬)
- `pbt/Cargo.toml` の `shiguredo_http2` を `[dev-dependencies]` から `[dependencies]` に移動 (feature 指定なし)

### 書き換えたもの

- `pbt/tests/prop_validation.rs` の 7 箇所の `header_field_from_validated_parts` を `pbt::wire_header_field` に置換
- `fuzz/fuzz_targets/fuzz_validation.rs` / `fuzz_hpack_roundtrip.rs` に wire 符号化ヘルパを定義し wire ヘルパ方式に移行
- `Cargo.toml` の `[dev-dependencies]` の `proptest` バージョンを `"1.11"` に更新
- issues 0036 / 0037 / 0039 の `__test_helpers` 関連記述を更新
- `CHANGES.md` の `### misc` に `[UPDATE]` エントリを追記

## 関連

- [[0024-change-header-field-construct-time-validation]] (`__test_helpers` 導入元)
- [[0033-refactor-dedupe-from-validated-parts-cfg]] (公開層を `__test_helpers` に集約)
- [[0036-refactor-move-mod-tests-to-tests-dir]] (`__test_helpers` 例外条項が本 issue で削除対象)
- [[0037-add-fuzz-build-check-to-ci]] (`__test_helpers` 記述が本 issue で更新対象)
- [[0039-fix-pbt-naming-convention]] (`__test_helpers` 完了条件が本 issue で更新対象)
