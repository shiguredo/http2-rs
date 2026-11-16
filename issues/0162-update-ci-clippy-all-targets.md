# CI の clippy に --all-targets を追加して Makefile と prek に揃える

- Created: 2026-09-13
- Completed: {YYYY-MM-DD}
- Branch: feature/update-ci-clippy-all-targets
- Polished: 2026-11-17

## 目的

CI の clippy だけが `--all-targets` を持たず、テストターゲットが lint されない状態を解消する。`Makefile` の `clippy` と `prek.toml` の `cargo-clippy` は `--all-targets` 付きなので、ローカルでは検出できるテストコードの lint 違反が CI では検出できない。

## 現状

- `.github/workflows/ci.yml` の clippy ステップは `cargo clippy --workspace -- -D warnings`
- `Makefile` の `clippy` と `prek.toml` の `cargo-clippy` は `cargo clippy --workspace --all-targets -- -D warnings`
- `--all-targets` の有無で検査対象が変わる。実測すると `cargo clippy -p interop_h2` は lib の 1 ユニットだけを検査し、`--all-targets` を付けると lib・lib のユニットテスト・統合テスト 2 ファイルの 4 ユニットを検査する
- issue 作成時に macOS で `cargo clippy --workspace --all-targets -- -D warnings` が警告 0 で通ることを確認済み。Linux x86_64 でも同コマンドが警告 0 で通ることを確認済み
- CI は `ubuntu-24.04` / `ubuntu-24.04-arm` / `macos-26` の 3 ランナーで `cargo test --workspace` を実行している

## 設計方針

- `.github/workflows/ci.yml` の clippy ステップを `cargo clippy --workspace --all-targets -- -D warnings` に変更し、`Makefile` / `prek.toml` と一致させる
- 他のランナー固有の警告が出ないことを CI で確認する。CI は `schedule` と `workflow_dispatch` で起動するので、確認は次回の定期実行または手動実行で行う
- `--all-targets` で新たに警告が見つかった場合は、本 issue の範囲で警告の解消に必要な最小限の修正を行い、CI を緑にする
- 依存・ツールチェーン・他のステップは変更しない

## 完了条件

- `.github/workflows/ci.yml` の clippy が `--all-targets` 付きで実行されること
- `Makefile` / `prek.toml` / `.github/workflows/ci.yml` の clippy コマンドが一致していること
- ローカルで `cargo clippy --workspace --all-targets -- -D warnings` が通ること
- CI の定期実行または手動実行で 3 ランナーすべての clippy が通ること
- ライブラリのコードとテストに変更が無いこと (ただし `--all-targets` で新たに検出された警告の解消に必要な最小限の修正を除く)
