# WtServerSession::close() がエラーを無視する問題を修正する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-close-error-propagation
- Polished: {YYYY-MM-DD}

## 目的

`WtServerSession::close()` が driver タスクへのコマンド送信エラーと応答受信エラーを無視している問題を修正し、呼び出し元にエラーを伝播させる。

## 現状

`WtServerSession::close()`（`crates/tokio-http2/src/webtransport.rs` の `WtServerSession` 型の `close` メソッド）では:

- `cmd_tx.send()` の結果を `let _ = ...` で無視している
- `rx.await` の結果も `let _ = ...` で無視している

driver が既に落ちている場合、`cmd_tx.send()` は `Err` を返すが無視される。`rx.await` も driver が応答を返せない場合に `Err` を返すが無視される。これにより、実際には close 処理が行われていないのに呼び出し元は `Ok(())` を受け取り、成功したと誤認する。

## 設計方針

- `cmd_tx.send()` のエラーを検出し、`ConnectionClosed` などの適切なエラーを返す
- `rx.await` のエラーも同様に処理する
- エラーを無視する意図がある場合は、コメントで明示する

## 完了条件

- driver が落ちている場合に `close()` がエラーを返すこと
- 正常系では従来通り `Ok(())` が返ること
- `cargo test -p tokio-http2` が全件通過すること
