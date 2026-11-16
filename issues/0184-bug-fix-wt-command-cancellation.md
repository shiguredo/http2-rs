# tokio WebTransport のコマンド API がキャンセル安全でない

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-command-cancellation
- Polished: {YYYY-MM-DD}

## 目的

`WtServerSession` / `WtSessionHandle` / `WtBidiStream` / `WtUniSendStream` の各操作は「`cmd_tx.send(...)` してから `oneshot` を await」する。`tokio::select!` や timeout で future を落とすと、driver はコマンドを処理するが ack の受信者がいないため結果は捨てられる。ストリームや送信枠がリークし、再試行でデータが二重送信される経路を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `open_bidi` / `open_uni` / `send_datagram` / `export_keying_material` / `close` / `drain` は `oneshot` で結果を待つ。`open_bidi` がキャンセルされると `DriverState::stream_channels` に送信者だけが残り、返却予定だった `WtBidiStream` は失われる。`open_uni` はストリーム ID と送信枠を消費したままハンドルが消える。`send` は送信後に成功を知らず、再試行すると同じデータを二重送信し得る。

## 設計方針

- 各メソッドに `# Cancel safety` を明記し、キャンセル時の状態を定義する。
- driver が `ack.is_closed()` を検出し、生成直後のストリームを reset / 削除する。
- またはリクエスト ID と冪等性を持たせ、再送・再開を可能にする。
- `send` は「未送信」「送信済み」を区別できる契約にする。
- キャンセルを再現するテスト (open / send / close の各操作) を追加する。

## 完了条件

- キャンセルされた `open_bidi` / `open_uni` でストリームや送信枠がリークしないこと。
- キャンセルされた `send` の再試行で二重送信が起きない契約になっていること。
- 各公開メソッドの rustdoc にキャンセル安全性が明記されていること。
- キャンセル経路のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
