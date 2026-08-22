# WtServerSession::close() がエラーを無視する問題を修正する

- Created: 2026-08-16
- Completed: 2026-08-22
- Branch: feature/fix-wt-close-error-propagation
- Polished: 2026-08-22

## 目的

`WtServerSession::close()` が driver タスクへのコマンド送信エラーと応答受信エラーを無視している問題を修正し、呼び出し元にエラーを伝播させる。

## 現状

`WtServerSession::close()`（`crates/tokio-http2/src/webtransport.rs` の `WtServerSession` 型の `close` メソッド）では、次の 3 箇所でエラーを無視している:

- `cmd_tx.send()` の結果を `let _ = ...` で無視している
- `rx.await` の結果を `let _ = ...` で無視している
- `driver.await` の結果を `let _ = ...` で無視している

`rx` は `oneshot::Receiver<Result<()>>` のため、`rx.await` は RecvError（driver が応答できない場合）と、ack で返される driver の処理結果（`wt_session.close()` の結果）の 2 段階のエラーを持つ。driver が既に落ちている場合、`cmd_tx.send()` は `Err` を返すが無視される。これにより、実際には close 処理が行われていないのに呼び出し元は `Ok(())` を受け取り、成功したと誤認する。

一方、`WtSessionHandle::close()`（同じ `webtransport.rs` の `WtSessionHandle` 型の `close` メソッド）は既にエラーを伝播している。

## 設計方針

- `cmd_tx.send()` のエラーを検出し、`Error::ConnectionClosed` を返す（他メソッドと同じ契約）
- `rx.await` が返す `Result<Result<(), Error>, RecvError>` の RecvError と、ack で返される `wt_session.close()` の結果の両方を伝播する（`WtSessionHandle::close()` と同一の契約）
- `driver.await` は ack 受信後に必ず `Ok(())` を返すため結果は無視するが、その理由をコメントで明示する

## 完了条件

- driver が落ちている場合に `close()` がエラーを返すこと
- driver の ack がエラーの場合（`wt_session.close()` の失敗）に `close()` がエラーを返すこと
- 正常系では従来通り `Ok(())` が返ること
- `cargo test -p tokio-http2` が全件通過すること

## 解決方法

`crates/tokio-http2/src/webtransport.rs` の `WtServerSession::close()` を修正した:

- `cmd_tx.send()` の結果を `.map_err(|_| Error::ConnectionClosed)?` で検査し、失敗時は `Error::ConnectionClosed` を返す（`open_bidi` / `drain` 等の他メソッドと同じ契約）
- `rx.await` が返す `Result<Result<(), Error>, RecvError>` の RecvError と ack 値（`wt_session.close()` の結果）の両方を伝播する
- `driver.await` は ack 受信後に必ず `Ok(())` を返すため無視するが、その理由をコメントで明示した
- `close()` の doc コメントに `# Errors` セクションを追記した

`crates/tokio-http2/tests/test_webtransport.rs` に、クライアントが CONNECT ストリームを END_STREAM で閉じて driver を終了させた後に `close()` を呼び、`Error::ConnectionClosed` が返ることを検証する E2E テスト `test_wt_close_errors_when_driver_dead` を追加した。

`CHANGES.md` の `## develop` に [FIX] エントリを追記した。
