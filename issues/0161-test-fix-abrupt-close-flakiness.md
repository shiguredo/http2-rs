# テストの応答後クローズによる確率的失敗を解消する

- Created: 2026-09-13
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-abrupt-close-flakiness
- Polished: {YYYY-MM-DD}

## 目的

テストが応答を送信した直後にサーバータスクを終了して接続を閉じるため、未読データが残っていると OS が RST を送り、クライアントが応答を受信できずに確率的に失敗する経路を解消する。

`interop/h2` の疎通確認テストでは同じ機構で失敗率 44/60 を実測しており、既存テストにも同型のパターンが残っている。既存テストの確率的な失敗は、CI や pre-push の失敗を実装の変更と誤認させる原因になる。

## 現状

- `interop/h2/src/lib.rs` の `drain_http2_connection` / `drain_nghttp2_connection` は、応答送信後にピアが接続を閉じるか `LINGER_TIMEOUT` (2 秒) まで `next_event` を回し続けて未読データを残さないようにしている。この対策を入れる前は `test_http2_client_reaches_nghttp2_server` が 44/60 で失敗し、失敗時のエラーは `Io(ConnectionReset)` だった
- 既存テストには「応答を送信したらサーバータスクが `break` して接続を閉じる」パターンが残っている
  - `crates/tokio-nghttp2/tests/client_server.rs`: `break;` が 76 箇所
  - `crates/tokio-http2/tests/client_server.rs`: `break;` が 91 箇所
  - `crates/tokio-http2/tests/interop.rs`: `break;` が 236 箇所
- `crates/tokio-http2/tests/interop.rs` には応答後に `tokio::time::timeout(Duration::from_secs(2), conn.next_event())` で待つ箇所が 3 箇所あるが、`next_event` はキュー済みのイベント (`FrameSent` など) を先に返すため、実際には待てていない
- 差分レビュー中に `cargo test --workspace` が 11 回中 1 回失敗し、`crates/tokio-nghttp2/tests/client_server.rs` の `test_basic_request_response` が `Io(Os { code: 54, kind: ConnectionReset })` を返した
- その後 `test_basic_request_response` を単体で通常負荷で 100 回、8 並列の CPU 負荷下で 60 回実行したが再現しなかった

## 設計方針

- まず再現条件を特定する。`cargo test --workspace` と `test_basic_request_response` を、負荷・並列度・実行順を変えて繰り返し実行し、失敗率を測る
- 原因は「サーバーが応答送信後に未読データを残したまま接続を閉じ、OS が RST を送る」ことである可能性が高い。`interop/h2` の `drain_*_connection` と同じ「ピアのクローズまで読む」対策を、既存テストのサーバー側ヘルパーにも適用する
- 同じ対策が複数ファイルに散らばらないよう、共有ヘルパー (`tests/helpers/`) への集約を検討する
- テストの検証内容・アサーション・テスト名は変えない
- 再現しなかった場合は、測定条件 (回数・負荷) と失敗率を issue に記録し、対策の必要性を改めて判断する

## 完了条件

- 再現条件の測定結果 (実行回数・負荷・失敗率) が issue に記録されていること
- 応答後のクローズが原因である場合、`interop/h2` と同じ「ピアのクローズまで読む」対策が既存テストに入っていること
- `cargo test --workspace` を 20 回以上連続実行して失敗 0 であること
- テストの検証内容・アサーション・テスト名が変わっていないこと
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること
