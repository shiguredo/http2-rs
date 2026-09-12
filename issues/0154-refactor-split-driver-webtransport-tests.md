# tokio-http2 の WebTransport テストを分割する

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-split-driver-webtransport-tests
- Polished: {YYYY-MM-DD}

## 目的

`crates/tokio-http2/tests/test_webtransport.rs` が 2832 行・32 テスト・12 箇所の同型イベントループを抱え、可読性と保守性が低下している。テストターゲットをディレクトリモジュール化し、関心ごとのサブモジュールへ分割したうえで、共有ヘルパーとイベントループを 1 箇所に集約する。テスト本文とアサーションの意味は変えない。

## 現状

`crates/tokio-http2/tests/test_webtransport.rs` は 2832 行、`#[tokio::test]` は 32 件である。

- クレート直下のヘルパーは `test_tls` / `connect_request` / `perform_connect` / `await_connect_headers` / `server_limits` / `client_limits_with_wt` / `asymmetric_server_limits` / `initial_one_server_limits` の 8 個で、テスト本体と同一ファイルに混在している
- 「CONNECT ストリームの `Event::DataReceived` を `WtSession::feed` へ渡し、`process` と `poll_event` を回して目的の状態になるまで繰り返す」ループが 12 箇所に重複している。ループの本体は同一で、終了条件だけが異なる
- 次の関心が 1 ファイルに混在している
  - bidi / uni のエコーとデータグラム
  - ストリーム / セッションのフロー制御と自動ウィンドウ拡張
  - WT_STOP_SENDING と WT_RESET_STREAM
  - WT_CLOSE_SESSION / END_STREAM とドライバ終了時のエラー
  - Origin の検証
  - TLS バージョンと証明書
  - WebTransport-Init ヘッダー
  - サブプロトコルとスキームの検証

`crates/tokio-http2/tests/` には分割の前例が無い (`client_server.rs` 3518 行、`interop.rs` 12288 行)。一方でクレート直下の `tests/test_webtransport/` は `main.rs` に `mod` を並べるディレクトリモジュール構成を既に採用している。

## 設計方針

- `crates/tokio-http2/tests/test_webtransport/` を新設し、`main.rs` に `mod` を並べる構成にする (クレート直下の `tests/test_webtransport/` と同じ構成)。既存の `tests/test_webtransport.rs` は削除する
- 分割の単位は現状に挙げた関心ごととし、ファイル名は内容が分かる英語ケバブケースにする
- 共有ヘルパーは `main.rs` に置き、各サブモジュールから `use crate::...` で参照する。ヘルパーの実装は変更しない
- 12 箇所のイベントループは、終了条件をクロージャで受け取る 1 個のヘルパーに集約する。集約するとテストの意図が読みにくくなる箇所はループを残し、その理由をコメントする
- テスト本文・アサーション・テスト名は変更しない。分割で変わるのは `use` の並びと `mod` 宣言だけを目標にする
- `cargo test -p tokio-http2 --test test_webtransport` のテスト件数が分割前 (32 件) と一致することを確認する

## 完了条件

- テストが関心ごとのサブモジュールに分割され、各サブモジュールが 500 行以下になっていること
- 共有ヘルパーが `main.rs` に集約され、テスト本体と混在していないこと
- 重複したイベントループが共通ヘルパーに集約されていること (残した箇所は理由がコメントされていること)
- テスト名とアサーションの意味が分割前と変わっていないこと
- `cargo test --all` が通過し、`test_webtransport` のテスト件数が 32 件のままであること
