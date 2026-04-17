# CHANGES.md を整備して WebTransport サーバーサンプル追加を仕上げる

- Created: 2026-04-17
- Model: Opus 4.7

## 概要

`CHANGES.md` に今回の変更履歴を記載し、親 issue 0001 を含む全サブ issue を `issues/closed/` に移動して完了状態にする。

## 背景

CLAUDE.md の変更履歴・issue 運用ルールに従い、リリースノートの整合性を確保する。

## 根拠

- CLAUDE.md 「変更点とリリースノートの整合性を確認すること」
- CLAUDE.md 「1 issue 完了ごとに 1 コミットすること」「Issue の完了日はファイルのタイトルの後に `Completed: YYYY-MM-DD` として記載すること」

## 対応内容

### CHANGES.md

- ファイルが存在しない場合は新規作成
- `## develop` セクションに以下を追加 (種別順: UPDATE → ADD → CHANGE → FIX)
  - `[ADD] shiguredo_http2 に WebTransport 関連 SETTINGS の送受信を実装する`
  - `[ADD] tokio-http2 に WebTransport サーバー API を実装する`
  - `[ADD] tokio-http2 に WebTransport DATAGRAM / 動的フロー制御を実装する`
  - `[ADD] examples/wt_server を追加する`
  - 担当者行は各エントリ下に `  - @voluntas`

### Issue のクローズ

- 各 issue ファイルの先頭に `Completed: 2026-04-17` を追記
- 「## 解決方法」セクションを追加し、何をどう実装したかを記述
- `git mv issues/000N-*.md issues/closed/`

### 動作確認

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cd examples/wt_server && cargo build`
- prek のフック (pre-commit, pre-push) が全て通る

## 完了条件

- CHANGES.md に 4 つの `[ADD]` エントリが記載されている
- 0001〜0009 すべてが `issues/closed/` に移動している
- 全チェックが通る

## 依存

- 0001〜0008 全て
