# shiguredo_http2 の WebTransport integration テストを分割する

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-split-webtransport-integration-tests
- Polished: {YYYY-MM-DD}

## 目的

`tests/test_webtransport/integration.rs` が 4244 行・99 テストを抱え、`WtSession` の複数の関心が 1 ファイルに混在して可読性と保守性が低下している。既にディレクトリモジュール化されている `tests/test_webtransport/` のサブモジュールへ関心ごとに分割する。テスト本文とアサーションの意味は変えない。

## 現状

`tests/test_webtransport/integration.rs` は 4244 行、`#[test]` は 99 件である。次の関心が混在している。

- セッション / ストリームの基本操作 (open、送受信、状態 getter、クローズ後のエラー)
- フロー制御 (WT_MAX_DATA / WT_MAX_STREAM_DATA / WT_MAX_STREAMS と各 BLOCKED capsule)
- WT_RESET_STREAM と WT_STOP_SENDING の送受信
- 方向検証 (送信専用 / 受信専用ストリームへの操作)
- 削除済み・未作成 ID とピア開始 bidi の暗黙作成
- 受信状態の検証 (`Recv` / `DataRecvd` / `DataRead` / `ResetRead`)
- 引数の範囲検証 (error code と varint 上限)

`tests/test_webtransport/main.rs` は `capsule` / `error` / `exporter` / `flow_control` / `init` / `protocols` / `root` / `stream` / `varint` の 9 サブモジュールを既に宣言しており、`integration.rs` だけが関心を束ねたままの状態になっている。クレート直下の `tests/test_webtransport/` のサブモジュールは 173 行 (`varint.rs`) から 628 行 (`capsule.rs`) に収まっている。

分割を扱う issue には `issues/0111-refactor-split-reset-stream-tests.md` (`tests/test_connection.rs` が対象) と `issues/0154-refactor-split-driver-webtransport-tests.md` (`crates/tokio-http2/tests/test_webtransport.rs` が対象) があるが、本ファイルはどちらの対象でもない。

## 設計方針

- `tests/test_webtransport/integration.rs` を削除し、関心ごとのサブモジュールに分割して `tests/test_webtransport/main.rs` の `mod` に追加する。ファイル名は内容が分かる英語ケバブケースにする
- テスト本文・アサーション・テスト名は変更しない。分割で変わるのは `use` の並びと `mod` 宣言だけを目標にする
- 共有ヘルパー (`decode_single_capsule` と状態を作るヘルパー群) は、複数のサブモジュールから使うものだけを `main.rs` に移す。1 つのサブモジュールでしか使わないものはそのファイルに残す
- `cargo test --test test_webtransport` のテスト件数が分割前 (99 件 + 他サブモジュール) と一致することを確認する
- 0111 / 0154 と対象ファイルが重ならないため、これらの完了を待たずに着手できる

## 完了条件

- `tests/test_webtransport/integration.rs` が削除され、各サブモジュールが 1 つの関心に対応していること
- 各サブモジュールが 500 行以下になっていること
- テスト名とアサーションの意味が分割前と変わっていないこと
- `cargo test --all` が通過し、`test_webtransport` のテスト件数が分割前と一致すること
