# 構築時検査の compile-fail テストを trybuild で整備する

Created: 2026-05-23
Model: Opus 4.7

## 概要

`HeaderField::from_static`, `Setting::*::from_static`, `WindowSize::from_static` 等の
`const fn` ベースの構築 API について、**不正リテラルがコンパイルエラーになることを
回帰テストで担保する**仕組みを導入する。

`trybuild` クレートを `[dev-dependencies]` として追加し、`tests/trybuild/` 配下に
意図的に失敗するソースを置き、コンパイル時 panic で fail することを CI で検証する。

## 背景

`const fn` での構築時検査は強力だが、以下のリグレッションが起きやすい:

- 検査ロジックを `const fn` から普通の `fn` にうっかり戻すと、コンパイル時検出が消える
- `panic!` の文言を変更すると、利用者向けエラーメッセージが劣化する
- `from_static` が「全ての不正リテラルでコンパイルエラーになる」状態を維持しているか、
  通常テストでは検証できない

`trybuild` は意図的にコンパイル失敗するソースのコンパイル結果 (stderr) を比較する
テストランナーで、Serde や thiserror が同様のリグレッション防止に採用している。

## 根拠

- 「コンパイル時に弾ける」が本ライブラリの差別化要素 (issue 0024 参照) であり、
  この性質を CI で守らないとサイレントに失われる
- `compile_fail` doctest でも同等の検証はできるが、エラーメッセージの厳密な比較ができない
- `trybuild` は `dev-dependencies` 限定で本体ビルドに影響しない

## 設計

### 依存追加

```toml
# Cargo.toml
[dev-dependencies]
trybuild = "1"
```

### テスト配置

```
tests/
  trybuild.rs                                # ランナー
  trybuild/
    header_field_uppercase.rs                # コンパイル失敗ソース
    header_field_uppercase.stderr            # 期待エラーメッセージ
    header_field_crlf_in_value.rs
    header_field_crlf_in_value.stderr
    setting_initial_window_overflow.rs
    setting_initial_window_overflow.stderr
    setting_max_frame_size_too_small.rs
    setting_max_frame_size_too_small.stderr
    stream_id_zero_for_client.rs
    stream_id_zero_for_client.stderr
    window_increment_zero.rs
    window_increment_zero.stderr
    limits_window_overflow.rs
    limits_window_overflow.stderr
```

### ランナー実装

```rust
// tests/trybuild.rs
#[test]
fn compile_fail_construct_time_validation() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/trybuild/*.rs");
}
```

### コンパイル失敗ソースの例

```rust
// tests/trybuild/header_field_uppercase.rs
use shiguredo_http2::HeaderField;

const _BAD: HeaderField = HeaderField::from_static(b"Host", b"example.com");

fn main() {}
```

```rust
// tests/trybuild/setting_initial_window_overflow.rs
use shiguredo_http2::{Setting, WindowSize};

const _BAD: WindowSize = WindowSize::from_static(u32::MAX);

fn main() {}
```

### エラーメッセージのバージョン依存性

`trybuild` の `.stderr` は rustc のバージョンに依存して微妙に変わる。
本リポジトリは `rust-toolchain.toml` で固定 (現在 1.88) しているため、
CI で同じバージョンが使われる限り再現性がある。

複数のコンパイルバージョンでテストする必要が出た場合は `TRYBUILD=overwrite cargo test`
で `.stderr` を再生成し直す運用にする。

## 影響範囲

- `Cargo.toml`: `[dev-dependencies] trybuild = "1"`
- `tests/trybuild.rs`: ランナー追加
- `tests/trybuild/*.{rs,stderr}`: ケースファイル群

## CHANGES.md エントリ

```
- [ADD] `trybuild` による compile-fail テストを追加し、`const fn from_static` 系の
  リテラル違反検出のリグレッションを CI で防止する
```

## 受け入れ条件

- 全 `const fn from_static` API について、少なくとも 1 件以上の compile-fail ケースが存在する
- `cargo test --test trybuild` で全ケースが期待通り fail (= テストとしては成功) する
- `.stderr` の期待値が rust-toolchain.toml の固定バージョンと一致している
- 既存の全テスト・PBT・fuzz が通る

## 依存

- [[0024-change-header-field-construct-time-validation]]
- [[0026-change-setting-construct-time-validation]]
- [[0027-change-frame-construct-time-validation]]
- [[0028-change-limits-builder-result]]
