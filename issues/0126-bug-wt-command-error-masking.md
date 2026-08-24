# driver コマンド処理の失敗原因が Error::ConnectionClosed に丸められる問題を修正する

- Created: 2026-08-22
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-command-error-masking
- Polished: 2026-08-24

## 目的

driver タスクのコマンド処理中に I/O エラー（出力フラッシュや END_STREAM 送信の失敗）が発生すると、呼び出し側は実際の原因ではなく常に `Error::ConnectionClosed` を受け取り、失敗原因が失われる問題を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::handle_cmd` は、コマンド処理の結果を ack で返す前に `flush_wt_output()` や `conn.send_data()` を `?` で呼んでいる。これらの I/O 操作が失敗すると ack が送信されず、呼び出し側（`WtServerSession::close()` や `WtSessionHandle` の各メソッド等）の `rx.await` が RecvError になり、`Error::ConnectionClosed` に丸められる。実際の失敗原因（I/O エラー等）が失われる。

対象は全コマンド（`SendStreamData` / `OpenBidi` / `OpenUni` / `SendDatagram` / `ResetStream` / `StopSending` / `Close` / `Drain`）。

## 設計方針

- コマンド処理の結果（成功・失敗）を ack に必ず載せて送信する。I/O 操作が失敗した場合も、そのエラーを ack に含めて呼び出し側に伝える
- 呼び出し側の `rx.await` は RecvError と ack 値のエラーを区別して伝播する（RecvError は driver 消失を表すため `Error::ConnectionClosed`、ack 値は実際のエラー）
- 具体的な実装方法（I/O 操作の結果を集約して ack を必ず送る等）は実装者判断

## 完了条件

- driver のコマンド処理中に I/O エラーが発生した場合、呼び出し側が `Error::ConnectionClosed` ではなく実際のエラーを受け取ること
- driver が消失している場合は従来通り `Error::ConnectionClosed` が返ること
- `cargo test -p tokio-http2` が全件通過すること
