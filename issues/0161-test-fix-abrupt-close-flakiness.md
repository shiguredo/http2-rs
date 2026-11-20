# テストのサーバー側クローズによる失敗を解消する

- Created: 2026-09-13
- Completed: 2026-11-20
- Branch: feature/fix-abrupt-close-flakiness
- Polished: 2026-11-17

## 目的

テストのサーバータスクが接続を閉じる前に、未読データや送信待ちの出力を残したまま終了するため、OS が RST を送ったり、SETTINGS ACK などの送信待ちのフレームが送信されなかったりして、クライアントが応答や ACK を受信できずに失敗する経路を解消する。

`interop/h2` の疎通確認テストでは同じ機構で失敗率 44/60 を実測しており、既存テストにも同型のパターンが残っている。既存テストの失敗には、確率的に発生するものと、単体実行で決定的に発生するものがある。これらは CI や pre-push の失敗を実装の変更と誤認させる原因になる。

## 現状

- `interop/h2/src/lib.rs` の `drain_http2_connection` / `drain_nghttp2_connection` は、応答送信後にピアが接続を閉じるか `LINGER_TIMEOUT` (2 秒) まで `next_event` を回し続けて未読データを残さないようにしている。この対策を入れる前は `test_http2_client_reaches_nghttp2_server` が 44/60 で失敗し、失敗時のエラーは `Io(ConnectionReset)` (Debug 表記) だった
- 既存テストには「サーバータスクが接続を閉じる前に `break` する」パターンが残っている。次の数は `break;` の総数であり、クライアント側の待機ループやエラー終了も含む
  - `crates/tokio-nghttp2/tests/client_server.rs`: `break;` が 76 箇所
  - `crates/tokio-http2/tests/client_server.rs`: `break;` が 91 箇所
  - `crates/tokio-http2/tests/interop.rs`: `break;` が 236 箇所
- `crates/tokio-http2/tests/interop.rs` には応答後に `tokio::time::timeout(Duration::from_secs(2), conn.next_event())` で待つ箇所が 3 箇所あるが、`next_event` はキュー済みのイベント (`FrameSent` など) を先に返すため、ピアのクローズまで待てていない
- `crates/tokio-http2/tests/interop.rs` の `test_http2_client_nghttp2_server_settings` は、nghttp2 サーバーのタスクが SETTINGS/ACK の受信後に `break` して接続を閉じるため、サーバーの SETTINGS ACK がクライアントに届かず、`received_ack` のアサーションで失敗する。2026-11-17 の確認では単体実行で 200 回中 200 回失敗し、`cargo test --workspace` でも 20 回中 20 回失敗した
- 差分レビュー中に `cargo test --workspace` が 11 回中 1 回失敗し、`crates/tokio-nghttp2/tests/client_server.rs` の `test_basic_request_response` が `Io(Os { code: 54, kind: ConnectionReset })` を返した
- その後 `test_basic_request_response` を単体で通常負荷で 100 回、8 並列の CPU 負荷下で 60 回実行したが再現しなかった (2026-11-17 の再確認でも 100 回 + 60 回で失敗 0)

## 設計方針

- 現状の測定で、`test_http2_client_nghttp2_server_settings` は単体で 200/200 失敗し、`test_basic_request_response` は 100+60 回では再現しなかった。前者は決定的、後者は確率的である。どちらも「サーバータスクが接続を閉じる前に未読データや送信待ちの出力を残す」機構で説明できる
- 原因は「サーバーが接続を閉じる前に未読データや送信待ちの出力を残す」ことである。`interop/h2` の 44/60 の実測と、`test_http2_client_nghttp2_server_settings` の 200/200 の失敗で確認できる。`interop/h2` の `drain_*_connection` と同じ「ピアのクローズまで読む」対策を、既存テストのサーバー側の処理 (各テスト内のサーバータスク) にも適用する
- 対象は応答送信後に `break` する箇所だけではない。SETTINGS ACK などの送信待ちのプロトコル応答を残したまま、または受信完了後に `break` して接続を閉じる箇所も含め、サーバータスクが接続を閉じる前に drain する
- `interop/h2` の drain 関数は private なので、そのままは共有できない。各クレートの `tests/` 配下に drain ヘルパーを置き、同一クレート内のテストで共有する (例: `crates/tokio-http2/tests/helpers.rs` / `crates/tokio-nghttp2/tests/helpers.rs`)
- テストの検証内容・アサーション・テスト名は変えない
- 再現しなかった場合も、測定条件 (回数・負荷) と失敗率を issue に記録する。`interop/h2` の 44/60 と同型のパターンが既存テストにあるため、対策は入れる

## 完了条件

- 再現条件の測定結果 (実行回数・負荷・失敗率) が issue に記録されていること
- `interop/h2` と同じ「ピアのクローズまで読む」対策が、既存テストのサーバータスク (応答送信後、SETTINGS ACK などの送信待ちの応答を残したまま、受信完了後に接続を閉じる箇所を含む) に入っていること
- `cargo test -p tokio-http2 --test interop test_http2_client_nghttp2_server_settings -- --exact` が通ること
- `cargo test --workspace` を 20 回以上、負荷・並列度・実行順を変えて連続実行して失敗 0 であること
- `test_basic_request_response` を通常負荷で 100 回、8 並列の CPU 負荷下で 60 回実行して失敗 0 であること
- テストの検証内容・アサーション・テスト名が変わっていないこと
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること

## 解決方法

- `tokio-http2` と `tokio-nghttp2` の正常系の接続テストで、サーバーの検証と応答送信の後に、保留中の出力を処理しながらピアの切断を待つ。クライアントは既存の検証後に接続を閉じてからサーバータスクを待つ。
- 受信専用のフロー制御テスト 2 件では、接続とストリームの受信ウィンドウを消費量に応じて補充する。サーバーは元の受信アサーションを通過してから正常 GOAWAY を送信し、クライアントの切断を待つ。
- 修正前の `develop` で相互運用の SETTINGS ACK アサーション失敗を単独で再確認した。`tokio-nghttp2` の `test_settings_exchange` も単独 10 回で 10 回失敗した。修正後は両方とも単独で通過した。
- `test_basic_request_response` は修正前後とも通常負荷で 100 回、CPU 負荷 8 プロセスで 60 回実行し、失敗は各 0 件だった。
- ワークスペーステストをテストスレッド数 8・16・24・32、異なる実行順、CPU 負荷 0・8 プロセスで連続 20 回実行し、失敗 0 件だった。通常のワークスペーステスト、フォーマット、Clippy も通過した。
