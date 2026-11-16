# WT 統合テストの固定 sleep を決定的な同期に置き換える

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-test-sleep-sync
- Polished: {YYYY-MM-DD}

## 目的

`crates/tokio-http2/tests/test_webtransport.rs` には、driver の処理完了を待つために 50 ms から 2 秒の固定 `tokio::time::sleep` を使う箇所が 20 以上ある。負荷の高い CI では sleep 明けに処理が終わっておらず失敗し、成功時も不要な待ち時間が発生する。決定的な同期に置き換えてフレーキーさと実行時間を解消する。

## 現状

driver の END_STREAM 処理、bidi の受信、close の完了などを固定 sleep で待っている。例えば `test_wt_close_errors_when_driver_dead` は 2 秒 sleep した後に `session.close()` が `ConnectionClosed` になることを期待するが、完了条件そのものを待っていない。

既存 open issue 0161 は「応答後クローズによる確率的失敗」を扱うが、固定 sleep 全般の同期方法は対象外である。

## 設計方針

- driver の `JoinHandle` を await する。
- `Notify` / `oneshot` / `mpsc` で処理完了を通知する。
- `tokio::time::timeout` と条件待ち (イベント、状態、チャネル) を組み合わせる。
- どうしても時間待ちが必要な箇所だけ、上限付き timeout に限定する。
- sleep を同期手段にしているテストを洗い出し、すべて置き換える。

## 完了条件

- WT 統合テストから固定 sleep による同期がなくなっていること。
- CI の負荷変動で失敗しない決定的な同期になっていること。
- テストの実行時間が不必要に増えていないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
