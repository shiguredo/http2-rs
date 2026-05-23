# `src/<module>` 内の `#[cfg(test)] mod tests` を `tests/test_<module>.rs` に移す

Created: 2026-05-23
Model: Opus 4.7

## 内容

CLAUDE.md「テストについて」(L78-L88) の規約「単体テストのファイル名は `tests/test_<module>.rs` とし、`src/<module>.rs` に対応させること」「`src/<module>/` のようにディレクトリモジュールの場合は `tests/test_<module>/main.rs` にサブモジュール対応で分割すること」に違反する `#[cfg(test)] mod tests` ブロックが現状 `src/` 配下の 20 ファイルに存在する。これらを移管原則 (後述) に従い `tests/` に移す。

## 現状

`grep -c '#\[test\]' src/...` で計測した実数。`pub(crate)` 直接参照は `grep -E "from_validated_parts|validate_field_name|validate_field_value|validate_pseudo_header|insert_validated"` のマッチ件数 (テスト本体での呼び出しを含み、関数定義行は除外)。

| ファイル | `#[test]` 件数 | `pub(crate)` 直接参照 | Phase |
|---|---:|---:|:-:|
| `src/decode_error.rs` | 4 | 0 | 1 |
| `src/send_error.rs` | 5 | 0 | 1 |
| `src/limits.rs` | 6 | 0 | 1 |
| `src/flow_control.rs` | 7 | 0 | 1 |
| `src/stream/buffer.rs` | 4 | 0 | 1 (注: `src/stream/` はディレクトリモジュールのため `tests/test_stream/{main.rs, buffer.rs}` 形式に配置する) |
| `src/hpack/integer.rs` | 6 | 0 | 2 |
| `src/hpack/huffman.rs` | 5 | 0 | 2 |
| `src/hpack/error.rs` | 7 | 0 | 2 |
| `src/hpack/encoder.rs` | 6 | 2 | 2 |
| `src/hpack/decoder.rs` | 7 | 4 | 2 |
| `src/validation.rs` | 24 | 6 | 3 |
| `src/stream_id.rs` | 18 | 8 | 3 |
| `src/hpack/dynamic_table.rs` | 6 | 2 | 3 |
| `src/hpack/table.rs` | 27 | 4 | 3 |
| `src/webtransport/mod.rs` | 8 | 0 | 4 |
| `src/webtransport/stream.rs` | 10 | 0 | 4 |
| `src/webtransport/capsule.rs` | 17 | 0 | 4 |
| `src/webtransport/flow_control.rs` | 11 | 0 | 4 |
| `src/webtransport/varint.rs` | 11 | 0 | 4 |
| `src/hpack/bytes.rs` | 6 | - | **対象外** (0035 で `bytes.rs` 自体が削除) |

合計 195 件。`bytes.rs` 6 件を除いた **19 ファイル・189 件** が本 issue の対象。

「`pub(crate)` 直接参照」列は実装本体と `#[cfg(test)] mod tests` 内の両方を含む grep ヒット数のため、テスト依存の正確判定には使えない (実装内呼び出しもカウントされる)。`fn _helper(...)` 形式の private ヘルパ依存も含めて、Phase 着手時に各ファイルを目視確認すること。

## 設計方針

### 移管原則

1. **公開 API 経由で再現できるテストは `tests/test_<module>.rs` に移す**。
2. **`pub(crate)` 関数を直接叩くテスト、または private ヘルパに依存するテストは `src/<module>` 内の `#[cfg(test)] mod tests` に残す**。テストのために API 表面を広げる方針 (`pub` 昇格、新規 feature 追加) は採らない。例外として `__test_helpers::header_field_from_validated_parts` (0033 で公開済み) を使うテストは crate 外に出せる。
3. **`#[cfg(test)] pub` で crate 外公開する選択肢は採用しない**。`#[cfg(test)]` 限定の `pub` は他 crate (integration test, fuzz, examples) から見えないため `pub(crate)` と挙動が変わらず、また「テスト時のみ pub」という cfg 揺れは可読性を下げる。本 issue は原則 2 (mod tests 残置) を優先する。
4. **どちらにも分類できないグレー判定 (例: `fn`-only テストヘルパに依存する 1 テストだけ tests/ に出す)** は、PR レビュー時に都度判断する。

### ディレクトリモジュールの扱い

`src/webtransport/` (`mod.rs`, `stream.rs`, `capsule.rs`, `flow_control.rs`, `varint.rs`) はディレクトリモジュール。Rust の integration test は `tests/test_webtransport/main.rs` が **テストバイナリのエントリ** となり、サブモジュールは `main.rs` 内で `mod stream;` 等の宣言が必要。以下の構造で配置する。

```
tests/test_webtransport/
├── main.rs            # mod 宣言と共通ユーティリティ (mod root; mod stream; ...)
├── root.rs            # src/webtransport/mod.rs 由来のテスト群 (top-level module は root.rs 命名)
├── stream.rs          # src/webtransport/stream.rs 由来
├── capsule.rs         # src/webtransport/capsule.rs 由来
├── flow_control.rs    # src/webtransport/flow_control.rs 由来
└── varint.rs          # src/webtransport/varint.rs 由来
```

各サブモジュール `.rs` ファイル内に `#[test]` を含める。`main.rs` 自体には `#[test]` を含めず、`mod` 宣言と共通ヘルパ (`mod common;` 等が必要なら) のみ置く。

### 既存 `tests/` ファイルの整理

- `tests/rfc7541.rs`: RFC 7541 仕様準拠テスト。CLAUDE.md L84「特定のモジュールに対応しないテストには `test_` プレフィックスを付けないこと」に従い命名は妥当。本 issue で触らない。
- `tests/test_webtransport.rs`: 既存の WebTransport 統合テスト。本 issue で `tests/test_webtransport/integration.rs` (or 既存内容を分解した複数 `.rs`) に移す。混在許容 (統合テストと単体テストを同ディレクトリに置く) で進め、後続整理は別 issue 化。

### Phase 分割 (任意の実装手順)

合計 189 件を 1 PR で移すと差分が膨大になりレビュー負担が大きい。実装者の裁量で以下の Phase に分割してよい (Phase 別 PR でも 1 PR でもよい)。Phase 番号は上記表の最終列に対応。

- **Phase 1**: トップレベル平場 (`decode_error`, `send_error`, `limits`, `flow_control`, `stream/buffer` の計 26 件)
- **Phase 2**: hpack 配下のうち `pub(crate)` 依存が浅い箇所 (`integer`, `huffman`, `error`, `encoder`, `decoder` の計 31 件)
- **Phase 3**: `pub(crate)` 直接依存が明確な箇所 (`validation`, `stream_id`, `dynamic_table`, `table` の計 75 件)
- **Phase 4**: webtransport ディレクトリモジュール (5 ファイル・計 57 件)

Phase 別 PR にする場合のブランチ命名は `feature/refactor-move-mod-tests-phase{1..4}`。1 PR にまとめる場合は `feature/refactor-move-mod-tests-to-tests-dir`。

## 完了条件

- [ ] 19 ファイル (`bytes.rs` を除く) の `#[cfg(test)] mod tests` のうち、移管原則 1 に該当するテストが `tests/test_<module>.rs` または `tests/test_<module>/<sub>.rs` に移っている
- [ ] 移管原則 2 (および 4) に該当するテストは `src/<module>` 内の `#[cfg(test)] mod tests` に残っている
- [ ] `tests/test_webtransport/main.rs` 構造で 5 サブモジュール (`root`, `stream`, `capsule`, `flow_control`, `varint`) が `mod` 宣言で参照されている
- [ ] 既存 `tests/test_webtransport.rs` の統合テスト内容が `tests/test_webtransport/integration.rs` (または相当ファイル) に移っている
- [ ] `tests/rfc7541.rs` は無変更
- [ ] 移管前後で `cargo test --workspace` の passed 件数が一致する (`cargo test --workspace 2>&1 | grep "^test result:" | awk '{p+=$4} END {print p}'` で集計)
- [ ] 移管前後で `cargo llvm-cov report` の対象モジュール行カバレッジが同等以上 (低下 0% を目視確認)
- [ ] Phase 3 着手時点で `tests/test_validation.rs` の件数が長大になった場合、PR レビューで CLAUDE.md L86「テストが長くなったらファイル内 `mod` で分割」適用要否を判断する (本完了条件は強制要件ではなく着手時判断項目)
- [ ] `cargo build` と `cargo build --features __test_helpers` の両方が通る
- [ ] `cargo test --workspace` と `cargo test --workspace --features __test_helpers` の両方が通る
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --all -- --check` が通る
- [ ] CHANGES.md `### misc` に下記文面を追記

## CHANGES.md エントリ

`## develop` の `### misc` に追記する:

```
- [UPDATE] `src/<module>` 内の `#[cfg(test)] mod tests` ブロックを `tests/test_<module>.rs` (および `tests/test_<module>/main.rs`) に分離する
  - @voluntas
```

## スコープ外

- `src/hpack/bytes.rs` 内の `mod tests` (6 件) → 0035 で `bytes.rs` 自体が削除される
- `src/hpack/table.rs` (27 件) / `src/validation.rs` (24 件) のモジュール分割 (テストではなく `src/` 側の分割) → 別 issue 化。本 issue 完了後に必要に応じて新規 issue を起こす (本 issue では起票は完了条件にしない)
- PBT で代替可能な単体テストの整理 → 物理位置移動とロジック整理は別関心事。本 issue は物理移動のみを行い、テストロジックは無変更。PBT 代替検討は別 issue
- PBT (`pbt/tests/`) の命名規約整備 → 0039 で対応
- `crates/tokio-http2/`, `crates/tokio-nghttp2/`, `crates/shiguredo_nghttp2/`, `crates/nghttp2-sys/` 配下の `#[cfg(test)] mod tests` → ルート crate のみが本 issue のスコープ

## テスト戦略

本 issue はテストファイルの物理位置を変えるだけで、テストロジックは無変更。以下を担保する。

- **件数一致**: `cargo test --workspace 2>&1 | grep "^test result:" | awk '{p+=$4} END {print p}'` で移管前後の合計 passed 件数が一致
- **カバレッジ低下なし**: CLAUDE.md L107-L121 のカバレッジ取得コマンドで移管対象モジュールの行カバレッジが同等以上
- **CI 通過**: `cargo test --workspace` および `cargo test --workspace --features __test_helpers` の両方を CI で確認
- **`#[ignore]` 禁止**: CLAUDE.md L85 規約。本 issue で `#[ignore]` を新規付与しない

## 依存

Phase ごとに blocking 関係が異なるため Phase 別に列挙する。

- **Phase 1 / Phase 2**: blocking 依存なし (`pub(crate)` 直接参照ゼロまたは軽微、検査関数の `src/syntax.rs` 配置にも未依存)。0033/0034/0035 完了前に着手可能
- **Phase 3**: blocking 依存
  - [[0033-refactor-test-helpers-module-and-bytes-mod-name]] (`__test_helpers::header_field_from_validated_parts` ラッパ完成後、tests/ から `HeaderField::from_validated_parts` 相当を呼べるようになる)
  - [[0034-refactor-consolidate-field-syntax-module]] (`validate_*` テストの crate path が `crate::syntax::` に確定している必要がある)
- **Phase 4**: blocking 依存なし (webtransport は `pub(crate)` 直接参照ゼロ)
- **`src/hpack/bytes.rs`**: [[0035-refactor-replace-header-bytes-with-cow]] (ファイル削除により本 issue のスコープから自動除外)
- 関連: [[0039-fix-pbt-naming-convention]] (PBT 側の同種命名整備。tests/ の命名規約と PBT 命名規約は CLAUDE.md で対応関係にあり、両 issue 完了で全テスト命名が規約準拠する)
