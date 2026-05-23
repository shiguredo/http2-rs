# CI で fuzz クレートのビルド確認を行う

Created: 2026-05-23
Model: Opus 4.7

## 内容

`.github/workflows/ci.yml` に fuzz クレートのビルド確認ステップを追加し、本リポジトリの API 変更によって fuzz_targets がコンパイル不能になる regression を CI で検出できるようにする。

## 背景

- `fuzz/` は `Cargo.toml` の `[workspace] exclude = ["fuzz"]` で workspace から除外されており、`cargo test --workspace` でビルド検査されない。
- issue 0024 の作業中、`HeaderField` API 変更により fuzz_targets が一時的にコンパイル不能になったが、CI では検出されず /review-diff-code 経由でようやく発覚した。

## 設計方針

- ci.yml に以下を追加:
  ```yaml
  - name: Check fuzz crate
    run: cargo check --manifest-path fuzz/Cargo.toml
  ```
- 可能なら `cargo fuzz check` または `cargo fuzz build` も実行する (nightly toolchain が必要なため judgment 要)。

## 完了条件

- [ ] `.github/workflows/ci.yml` に fuzz クレートのビルド確認ステップが追加されている
- [ ] CI が緑で通る
- [ ] CHANGES.md `### misc` に変更を追記

## 依存

なし
