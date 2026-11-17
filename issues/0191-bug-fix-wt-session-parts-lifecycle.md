# `WtSessionParts` の driver タスクが終了せず、Drop の doc も実装と異なる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-session-parts-lifecycle
- Polished: 2026-11-17

## 目的

`WtSessionParts` を `WtServerSession::into_parts` で取り出した後、利用者が `WtSessionParts` を drop しても driver タスクと TCP 接続が残る。`DriverState` が `cmd_tx` を自己保持するため、外部の sender がすべて drop されても `cmd_rx.recv()` が `None` にならない。さらに `WtSessionParts::driver` の doc は「drop で abort される」と書くが、`JoinHandle` の drop は detach であり abort ではない。タスクと接続のライフサイクルを明確にする。

本 issue では `WtSessionParts` に Drop を追加せず、`DriverState` の強い自己 sender を `WeakUnboundedSender` に置き換えることで、`WtSessionParts` に由来する外部 sender (`WtSessionHandle` と各ストリーム) がすべて drop されたときに driver が自然終了するようにする。`WtSessionParts` の `driver` は公開フィールドのまま残し、利用者が `await` / `abort` できるようにする。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept()` は `driver_cmd_tx = cmd_tx.clone()` を作り、`DriverState { cmd_tx: driver_cmd_tx, ... }` に渡す。`DriverState` は `cmd_tx: mpsc::UnboundedSender<DriverCmd>` を保持し、`DriverState::handle_cmd` の `OpenBidi` / `OpenUni` と `DriverState::dispatch_wt_event` の `StreamOpened` で `self.cmd_tx.clone()` を新しいストリームへ渡す。このため `DriverState` 自身が強い sender を保持し続け、`WtSessionHandle` (clone を含む) と各ストリームの `cmd_tx` がすべて drop されても `cmd_rx.recv()` は `None` にならない。

`WtServerSession` は `Drop` で `driver.abort()` する。`WtSessionParts` には `Drop` がなく、`driver` の `JoinHandle` を drop しても abort されない。`WtSessionParts::driver` の doc は「driver タスクの JoinHandle。drop で abort される。」と書くが、`JoinHandle` の drop は detach である。`WtServerSession` の doc は「drop 時は cmd_tx が閉じ、driver が自然に終了する」と書くが、実装は `driver.abort()` である。

`WtSessionParts` は全フィールドが `pub` であり、`examples/wt_server/src/main.rs` の `run_echo` は `let WtSessionParts { ... driver, .. } = parts;` と値で分割代入し、`crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_max_data_non_increase_aborts_session` は `parts.driver.await` で `driver` をムーブしている。`WtSessionParts` に `Drop` を実装すると、これらのムーブアウトがコンパイルできなくなる。

## 設計方針

- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept()` で `cmd_tx.downgrade()` を作り、`DriverState` の `cmd_tx` フィールドを `mpsc::WeakUnboundedSender<DriverCmd>` に置き換える。`DriverState` は強い sender を保持しないため、`WtSessionHandle` (clone を含む) と各ストリームの `cmd_tx` がすべて drop されると `DriverState::run` の `cmd_rx.recv()` が `None` を返し、ループが終了する。driver が所有する `ServerConnection` の drop により TCP 接続も閉じる。
- `WtBidiStream` / `WtUniRecvStream` / `WtUniSendStream` を生成するときは `WeakUnboundedSender::upgrade()` で強い sender を取得して渡す。`upgrade()` が `None` を返した場合は外部 sender がすべて drop 済みなので、ストリームを開かずに処理を打ち切り、driver のループを終了する。`OpenBidi` / `OpenUni` の ack が未送信なら `Error::ConnectionClosed` を返す。
- `WtSessionParts` に Drop も明示的な shutdown API も追加しない。`WtSessionParts` を drop すると `handle` の `cmd_tx` が drop され、他の外部 sender (`WtSessionHandle` の clone / 各ストリーム) が残っていなければ driver は自然終了する。`driver` の `JoinHandle` の drop は detach であり abort ではないが、driver は上記の自然終了で停止する。`WtSessionParts` を値で分割代入して `driver` を取り出し、`await` / `abort` できる現行の利用方法を維持する (公開フィールドのムーブアウトを壊さない)。
- `WtServerSession` の Drop は現行どおり `driver.abort()` を呼ぶ。`WtServerSession` の doc を次のように修正する。`close()` は成功時に driver の終了を待つ。drop は、driver を保持している場合に driver を abort する。`into_parts()` で driver の所有権を `WtSessionParts` に移した場合、`WtServerSession` の drop は driver を abort しない。
- `WtSessionParts` / `WtSessionParts::driver` / `WtSessionHandle` の doc を修正する。`WtSessionParts` を drop すると `handle` の sender が drop され、他の外部 sender がなければ driver が自然終了すること、`WtSessionParts::driver` の `JoinHandle` の drop は detach であること、外部 sender が残っている間は外部 sender の drop だけでは driver が終了しないこと、`WtServerSession` の drop は abort することを明記する。`WtSessionHandle` には、driver が終了している場合に送信系 API が `Error::ConnectionClosed` を返すことを明記する。
- `WtSessionParts` を drop したときに driver タスクと接続が終了することを検証するテストを追加する。`WtSessionParts` をそのまま drop する経路では、クライアント側で接続終了を確認する。`into_parts` で `driver` を取り出し、`handle` を drop した後に `driver.await` が `JoinError` にならず、driver が `Ok(())` で終了することを確認する。`WtSessionParts` を drop しても `WtSessionHandle` の clone が残っていれば、外部 sender の drop だけでは driver は終了しないことも検証する。待機は `tokio::time::timeout` で上限を設け、固定 sleep に依存しない。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しない。

## 完了条件

- `WtSessionHandle` (clone を含む) と各ストリーム (`WtBidiStream` / `WtUniRecvStream` / `WtUniSendStream`) の `cmd_tx` がすべて drop されると `cmd_rx.recv()` が `None` を返し、driver タスクが終了して TCP 接続が閉じること。`WtSessionParts` を drop した場合も、他の外部 sender が残っていなければ同様に終了すること。
- `DriverState` が強い `cmd_tx` を保持していないこと。
- `WtSessionParts` に Drop を追加しておらず、`driver` を公開フィールドのまま `await` / `abort` できること。
- `WtServerSession` / `WtSessionParts` / `WtSessionParts::driver` / `WtSessionHandle` の doc が実装と一致していること。`WtServerSession` の drop は abort、`WtSessionParts::driver` の drop は detach であり、driver の終了は外部 sender の drop による自然終了であることが明記されていること。
- 上記のライフサイクルのテストが追加されていること。
- `CHANGES.md` を変更していないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。

## 他 issue との関係

- `issues/0120-add-wt-event-user-notification.md` — `WtSessionParts` に `session_event_rx` を追加する。本 issue は Drop も明示的な shutdown API も追加しないため、`examples/wt_server/src/main.rs` の値分割代入は壊れない。0120 側で `..` による無警告破棄を避けるため `session_event_rx` を明示的に束縛する (0120 の設計どおり)。0120 の「Drop 追加 (または明示的な shutdown API の追加)」という記述は、本 issue ではどちらも追加しないため、0120 側で必要に応じて更新する。
- `issues/0140-test-add-wt-stream-state-error-e2e-test.md` / `issues/0200-bug-fix-wt-notification.md` — `WtSessionParts::driver` を await して driver の終了結果を検証する計画を維持できる。本 issue は `driver` を公開フィールドのまま残す。
- `issues/0190-change-wt-multi-session.md` — dispatcher のセッション登録解除は、driver の `JoinHandle` の終了または外部 sender の drop を契機にできる。本 issue の自然終了と整合させる。

## 参照

- `crates/tokio-http2/src/webtransport.rs` — `WtServerRequest::accept` / `DriverState` / `DriverState::run` / `DriverState::handle_cmd` / `DriverState::dispatch_wt_event` / `WtServerSession` / `WtSessionParts` / `WtSessionHandle`
- `crates/tokio-http2/tests/test_webtransport.rs` — `test_wt_max_data_non_increase_aborts_session` と新規ライフサイクルテスト
- `examples/wt_server/src/main.rs` — `run_echo` の `WtSessionParts` 分割代入
- `issues/0120-add-wt-event-user-notification.md` / `issues/0140-test-add-wt-stream-state-error-e2e-test.md` / `issues/0190-change-wt-multi-session.md` / `issues/0200-bug-fix-wt-notification.md` — 関連 issue
