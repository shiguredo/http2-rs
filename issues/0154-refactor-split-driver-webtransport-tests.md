# tokio-http2 の WebTransport テストを分割する

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-split-driver-webtransport-tests
- Polished: 2026-11-16

## 目的

`crates/tokio-http2/tests/test_webtransport.rs` が 3187 行・35 テスト・15 箇所の同型イベントループ (ほかに `feed` と `process` だけを回す 2 箇所) を抱え、可読性と保守性が低下している。テストターゲットをディレクトリモジュール化し、関心ごとのサブモジュールへ分割したうえで、共有ヘルパーとイベントループを 1 箇所に集約する。テストの意味とアサーションは変えない。

## 現状

`crates/tokio-http2/tests/test_webtransport.rs` は 2026-11-16 時点で 3187 行、`#[tokio::test]` は 35 件である (35 件とも `cargo test -p tokio-http2 --test test_webtransport` で通過する)。

- クレート直下のヘルパーは 12 個で、テスト本体と同一ファイルに混在している
  - `connect_request` / `perform_connect` / `await_connect_headers` / `test_tls` / `server_limits` / `client_limits_with_wt` / `asymmetric_server_limits` / `initial_one_server_limits`
  - `connect_request_with_webtransport_init` / `connect_request_with_origin` / `connect_request_with_protocols` / `connect_request_with_scheme`
- 「CONNECT ストリームの `Event::DataReceived` を `WtSession::feed` へ渡し、`process` と `poll_event` を回して目的の状態になるまで繰り返す」ループが 15 箇所に重複している。`feed` / `process` / `poll_event` の呼び出し部分は同一で、イベントの処理と終了条件が異なる。このほかに `feed` と `process` だけを回すループが 2 箇所ある
- 次の関心が 1 ファイルに混在している
  - セッションの拒否 (CONNECT の 404 応答)
  - bidi / uni のエコーとデータグラム
  - ストリーム / セッションのフロー制御と自動ウィンドウ拡張
  - WT_STOP_SENDING と WT_RESET_STREAM
  - WT_CLOSE_SESSION / END_STREAM、drain、ドライバのコマンド失敗と終了時のエラー
  - Origin の検証
  - TLS バージョンと鍵素材のエクスポート
  - WebTransport-Init ヘッダー
  - サブプロトコルとスキームの検証

`crates/tokio-http2/tests/` には分割の前例が無い (`client_server.rs` 3518 行、`interop.rs` 12288 行)。一方でリポジトリルートの `tests/test_webtransport/` は `main.rs` に `mod` を並べるディレクトリモジュール構成を既に採用している。

## 設計方針

- `crates/tokio-http2/tests/test_webtransport/` を新設し、`main.rs` に `mod` を並べる構成にする (リポジトリルートの `tests/test_webtransport/` と同じ構成)。`main.rs` には `#[tokio::test]` を置かず、`mod` 宣言とヘルパーだけを置く。既存の `crates/tokio-http2/tests/test_webtransport.rs` は削除する
- 分割の単位は現状に挙げた関心ごとを基本とし、ファイル名は内容が分かる英語スネークケースにする。500 行を超える関心は、テストの意味を変えずにさらに分割する (例えばフロー制御は約 848 行あるため、セッション / ストリーム送信 / ストリーム受信 / ストリーム数の 4 分割が必要)
- 現状に挙げた 12 個のヘルパーは `main.rs` に置き、各サブモジュールから `use crate::...` で参照する。ヘルパーの実装は変更しない
- クレートレベルの lint 属性 (`#![allow(...)]` / `#![expect(...)]`) は `main.rs` に移す
- 15 箇所のイベントループは、終了条件をクロージャで受け取る 1 個のヘルパーに集約する。`feed` と `process` だけを回す 2 箇所も、同じヘルパーで扱えるなら寄せる。集約するとテストの意図が読みにくくなる箇所はループを残し、その理由をコメントする
- テストの意味・アサーション・テスト名は変えない。イベントループのヘルパー呼び出しへの置き換えと、`use` の並び・`mod` 宣言・クレートレベル属性の移動は行う
- `cargo test -p tokio-http2 --test test_webtransport` のテスト件数が分割前 (35 件) と一致することを確認する

## 完了条件

- テストが関心ごとのサブモジュールに分割され、各サブモジュールが 500 行以下になっていること (500 行を超える関心はさらに分割する)
- 現状に挙げた 12 個のヘルパーが `main.rs` に集約され、`main.rs` に `#[tokio::test]` が無いこと
- 15 箇所のイベントループのうち共通ヘルパーに集約できるものが集約されていること (残した箇所は理由がコメントされていること)
- テスト名とアサーションの意味が分割前と変わっていないこと
- `cargo test --all` が通過し、`test_webtransport` のテスト件数が 35 件のままであること
