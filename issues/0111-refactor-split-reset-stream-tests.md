# reset_stream テストモジュールを分割する

- Created: 2026-08-10
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-split-reset-stream-tests
- Polished: {YYYY-MM-DD}

## 目的

`tests/test_connection.rs` の `mod reset_stream` が約 3100 行に肥大し、可読性と保守性が低下している。shiguredo-rust スキルの「テストファイルが長くなった場合はファイル内で `mod` を使って分割すること」に従い、関心ごとに分割する。

## 現状

`tests/test_connection.rs` (約 3900 行) の `mod reset_stream` は、ストリームリセット関連のテストが 1 つのモジュールに集約されている。以下が混在している:

- ヘッダー経路のエラー処理テスト (`process_headers` の検証エラー・状態遷移エラー・状態遷移後 malformed)
- DATA 経路のエラー処理テスト (`handle_data` の Content-Length 不一致・no-content 違反・状態遷移違反)
- フロー制御違反テスト
- 接続ウィンドウ消費・補充テスト
- RST_STREAM / GOAWAY / 遅延フレーム破棄テスト

共通ヘルパー (`setup_server` / `setup_client` / `assert_headers_reset_events` / `assert_internal_reset` / `assert_delayed_data_discarded` 等) も同一モジュール内にあり、テスト本体と混在している。

## 設計方針

- 関心ごとにサブモジュールへ分割する (例: `mod reset_headers` / `mod reset_data` / `mod flow_control` / `mod window` 等)
- テスト間で共有するヘルパー (セットアップ・アサーションヘルパー) は shiguredo-rust スキルの規約に従い `tests/helpers/` へ移動する
- テストの内容・検証ロジックは変更せず、配置の再構成のみ行う

## 完了条件

- `mod reset_stream` が関心ごとのサブモジュールに分割され、共有ヘルパーが `tests/helpers/` に整理される
- テスト内容は変更されず、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `tests/test_connection.rs` — `mod reset_stream`
- shiguredo-rust スキル — テスト (テストファイル分割・`tests/helpers/` の規約)
