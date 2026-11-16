# tokio WebTransport ドライバの `next_event` キャンセルで HTTP/2 フレームが破損する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-next-event-cancellation
- Polished: {YYYY-MM-DD}

## 目的

`tokio-http2` の WebTransport ドライバは `tokio::select!` でコマンド受信と `ServerConnection::next_event()` を同時に待つ。`next_event()` の先頭で行われる送信フラッシュがキャンセル安全でないため、コマンド到着時に HTTP/2 のフレームが欠落または部分送信され、接続がプロトコルエラーになる経路を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::run` は `cmd_rx.recv()` と `conn.next_event()` を `select!` で待つ。`next_event()` は `Connection::flush()` を呼び、`flush()` は `poll_output()` で内部出力バッファを drain したうえで `write_all()` する。`write_all()` はキャンセル安全ではないため、コマンド到着で `next_event()` が drop されると、drain 済みのバイト列が失われ、途中まで書いたフレームの残りも送信されない。

出力バッファは drain 後に復元されないため、再送も行われない。バックプレッシャー下で `WtBidiStream::send` や `WtSessionHandle::close` が同時に呼ばれると、HTTP/2 のフレーム境界が壊れる。

## 設計方針

- `Connection::flush()` をキャンセル安全にする。送信待ちバイト列と書き込み offset を保持し、`write` の完了分だけ offset を進める。キャンセルされても未送信分を次回に引き継ぐ。
- または、読み取りタスクと書き込みタスクを分離し、`next_event()` を `select!` の対象にしない。
- `next_event()` の doc にキャンセル安全性の契約を明記する。
- バックプレッシャー中にコマンドを到着させる統合テストを追加し、フレーム欠落・部分送信が起きないことを検証する。

## 完了条件

- コマンド到着で `next_event()` がキャンセルされても、未送信の HTTP/2 フレームが欠落しないこと。
- `crates/tokio-http2/src/connection.rs` の `flush()` またはドライバのタスク構成がキャンセル安全になっていること。
- バックプレッシャー下で `WtBidiStream::send` と `close` を並行実行する回帰テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
