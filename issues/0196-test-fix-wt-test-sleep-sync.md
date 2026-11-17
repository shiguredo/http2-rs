# WT 統合テストの固定 sleep を決定的な同期に置き換える

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-test-sleep-sync
- Polished: 2026-11-17

## 目的

`crates/tokio-http2/tests/test_webtransport.rs` には、driver の処理完了を待つため、またはクライアントが応答を観測するまで driver を生かすために、50 ms から 2 秒の固定 `tokio::time::sleep` を使う箇所が 24 箇所ある。固定 sleep は条件を待たないため、負荷の高い CI では sleep 明けに処理が終わっておらず失敗し得る。成功時も不要な待ち時間が発生する。決定的な同期に置き換えてフレーキーさと実行時間を解消する。

## 現状

driver の END_STREAM 処理、bidi の受信、close の完了などを固定 sleep で待っている。例えば `test_wt_close_errors_when_driver_dead` は 2 秒 sleep した後に `session.close()` が `ConnectionClosed` になることを期待するが、driver の終了条件そのものを待っていない。`test_wt_bidi_echo` / `test_wt_uni_echo` / `test_wt_datagram_echo` などは、送信 API の完了後に 50 ms から 100 ms sleep して driver を生かし、クライアントが応答を読む時間を確保している。

`WtServerSession` の driver は `into_parts()` で `WtSessionParts::driver` として取り出せる。`WtServerSession::close()` は driver の終了を待つ。`into_parts()` 後は `WtSessionHandle` で送信系 API を呼べ、`WtSessionHandle::close()` は driver が終了していれば `ConnectionClosed` を返す。driver は接続終了、`close()`、またはエラーで終了するため、driver を await する箇所は終了条件を満たしてから await する。

既存 open issue 0161 はサーバータスクが接続を閉じる前に未読データや送信待ちの出力を drain する問題を扱い、固定 sleep の置き換えは対象外である。既存 open issue 0191 は `WtSessionParts` の driver 終了条件を変更するが、本 issue は固定 sleep を置き換える箇所の driver だけを対象にするため、0191 の完了を待たなくても実装できる。

## 設計方針

- 変更対象は `crates/tokio-http2/tests/test_webtransport.rs` のみとする。`crates/tokio-http2/tests/interop.rs` など他のテストの sleep は対象外とする。
- driver が終了するテストでは、driver の終了結果を確認する場合は `into_parts()` で取り出した `WtSessionParts::driver` を await し、`close()` の結果を確認する場合は `WtServerSession::close()` または `into_parts()` 後の `WtSessionHandle::close()` を使う。driver は接続終了、`close()`、またはエラーで終了するため、await する前に終了条件を満たす。
- driver を生かしたままクライアントの観測を待つテストでは、クライアント側の完了を `oneshot` / `mpsc` / `Notify` でサーバータスクへ通知し、通知後に session を drop または close する。固定 sleep で driver を生かさない。
- `tokio::time::timeout` と条件待ち (イベント、状態、チャネル) を組み合わせる。timeout は条件が満たされない場合の上限としてのみ使い、sleep の代わりに固定時間待ちをしない。
- sleep を同期手段にしている 24 箇所を洗い出し、すべて置き換える。
- 変更履歴は `CODEBASE.md` の指示に従い `CHANGES.md` に残さない。

## 完了条件

- `crates/tokio-http2/tests/test_webtransport.rs` から固定 sleep による同期がなくなっていること。
- 各テストが条件待ちまたは driver の終了待ちになっており、CI の負荷変動で失敗しない決定的な同期になっていること。
- `cargo test -p tokio-http2 --test test_webtransport` を CPU 負荷をかけた状態で 20 回以上実行して失敗 0 であること。
- テストの実行時間が不必要に増えていないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` が通ること。
- `cargo test --workspace` が通ること。ただし既存 open issue 0161 の `test_http2_client_nghttp2_server_settings` は現行 `develop` で失敗するため、0161 の修正後に確認する。
