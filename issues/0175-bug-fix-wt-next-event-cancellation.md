# tokio WebTransport ドライバの `next_event` キャンセルで HTTP/2 フレームが破損する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-next-event-cancellation
- Polished: 2026-11-17

## 目的

`tokio-http2` の WebTransport ドライバは `tokio::select!` でコマンド受信と `ServerConnection::next_event()` を同時に待つ。`next_event()` の先頭で行われる送信フラッシュがキャンセル安全でないため、コマンド到着時に HTTP/2 のフレームが欠落または部分送信され、接続がプロトコルエラーになる経路を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::run` は `biased` 指定の `select!` で `cmd_rx.recv()` と `conn.next_event()` を待つ。`biased` によりコマンド分岐が優先されるため、コマンドが到着すると `next_event()` は drop される。`next_event()` は `Connection::flush()` を呼び、`flush()` は `poll_output()` で内部出力バッファを drain したうえで `write_all()` する。`write_all()` はキャンセル安全ではないため、コマンド到着で `next_event()` が drop されると、drain 済みのバイト列が失われ、途中まで書いたフレームの残りも送信されない。

出力バッファは drain 後に復元されないため、再送も行われない。バックプレッシャー下で `next_event()` のフラッシュが待機しているときに `WtBidiStream::send` や `WtSessionHandle::close` のコマンドが到着すると、HTTP/2 のフレーム境界が壊れる。なお、本 issue は `next_event()` のフラッシュキャンセルに閉じ、0183 の送信バックプレッシャーと 0184 のコマンド API のキャンセル安全は別 issue として扱う。

## 設計方針

- 第一案として `Connection::flush()` をキャンセル安全にする。`Connection` に送信待ちバイト列と書き込み offset を保持するフィールドを追加し、`write` の完了分だけ offset を進める。キャンセルされても未送信分を次回の `flush()` に引き継ぎ、`write` が 0 バイトを返した場合は `WriteZero` としてエラーにする。
- 第二案として、読み取りタスクと書き込みタスクを分離し、`next_event()` を `select!` の対象にしない構成も許容する。どちらを選ぶ場合も、キャンセル時に未送信の HTTP/2 フレームを失わないことを共通の条件とする。
- `Connection::flush()` / `Connection::next_event()` と、それらを委譲する `ServerConnection` / `Client` の `flush()` / `next_event()` の doc にキャンセル時の契約を明記する。第一案ではキャンセル時は未送信出力を保持して次回の `flush()` / `next_event()` で送信を継続する旨を、第二案では `flush()` のキャンセル時の挙動を明記する。
- バックプレッシャー中にコマンドを到着させる統合テストを追加し、フレーム欠落・部分送信が起きないことを検証する。クライアントが読み取りを停止した状態で `WINDOW_UPDATE` を送り、サーバーが `WINDOW_UPDATE` の処理で生成した出力をフラッシュしている間に、`WtServerSession::into_parts()` で取得した `WtSessionHandle` の `close()` を `WtBidiStream::send()` と並行して呼ぶ。クライアント側で HTTP/2 フレームのデコードエラーが起きず、送信した WT データと END_STREAM が到着することを確認する。

## 完了条件

- コマンド到着で `next_event()` がキャンセルされても、未送信の HTTP/2 フレームが欠落しないこと。
- `crates/tokio-http2/src/connection.rs` の `flush()` またはドライバのタスク構成がキャンセル安全になっていること。
- `Connection::flush()` / `Connection::next_event()` と、それらを委譲する `ServerConnection` / `Client` の doc にキャンセル時の契約が明記されていること。
- バックプレッシャー下で `WtBidiStream::send` と `WtSessionHandle::close` を並行実行する回帰テストが追加され、クライアント側で HTTP/2 フレームのデコードエラーが起きず、送信した WT データと END_STREAM が到着することが確認されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
