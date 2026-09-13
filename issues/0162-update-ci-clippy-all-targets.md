# CI の clippy に --all-targets を追加して Makefile と prek に揃える

- Created: 2026-09-13
- Completed: {YYYY-MM-DD}
- Branch: feature/update-ci-clippy-all-targets
- Polished: {YYYY-MM-DD}

## 目的

CI の clippy だけが `--all-targets` を持たず、テストターゲットが lint されない状態を解消する。`Makefile` の `clippy` と `prek.toml` の `cargo-clippy` は `--all-targets` 付きなので、ローカルでは検出できるテストコードの lint 違反が CI では検出できない。

## 現状

- `.github/workflows/ci.yml` の clippy ステップは `cargo clippy --workspace -- -D warnings`
- `Makefile` の `clippy` と `prek.toml` の `cargo-clippy` は `cargo clippy --workspace --all-targets -- -D warnings`
- `--all-targets` の有無で検査対象が変わる。実測すると `cargo clippy -p interop_h2` は lib の 1 ユニットだけを検査し、`--all-targets` を付けると lib とテスト 2 ファイルの 4 ユニットを検査する
- macOS では `cargo clippy --workspace --all-targets -- -D warnings` が警告 0 で通る
- CI は `ubuntu-24.04` / `ubuntu-24.04-arm` / `macos-26` の 3 ランナーで `cargo test --workspace` を実行している

## 設計方針

- `.github/workflows/ci.yml` の clippy ステップを `cargo clippy --workspace --all-targets -- -D warnings` に変更し、`Makefile` / `prek.toml` と一致させる
- 他のランナー固有の警告が出ないことを CI で確認する。CI は `schedule` 起動のみなので、確認は次回の定期実行または手動実行で行う
- 依存・ツールチェーン・他のステップは変更しない

## 完了条件

- `.github/workflows/ci.yml` の clippy が `--all-targets` 付きで実行されること
- `Makefile` / `prek.toml` / `.github/workflows/ci.yml` の clippy コマンドが一致していること
- ローカルで `cargo clippy --workspace --all-targets -- -D warnings` が通ること
- CI の定期実行で 3 ランナーすべての clippy が通ること
- ライブラリのコードとテストに変更が無いこと
