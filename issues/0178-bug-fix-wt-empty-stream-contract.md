# 空 WT_STREAM capsule の送受信契約を仕様に合わせる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-empty-stream-contract
- Polished: {YYYY-MM-DD}

## 目的

draft-ietf-webtrans-http2-16 Section 6.4 は「空の WT_STREAM capsule はストリームを開くか閉じる場合にのみ使用してはならない (MUST NOT)」と定める。実装は既存ストリームへ空の非 FIN WT_STREAM を送信でき、受信側の空 FIN も呼び出し側に通常データとして渡るため、送受信の契約を仕様に合わせる。

## 現状

`src/webtransport.rs` の `WtSession::send_stream_data` は `data.is_empty()` を検査せず、`fin = false` でも `Capsule::WtStream` を生成する。`WtStream::send_data` はサイズ 0 でも送信状態を `Send` に進めるため、同じストリームへの 2 回目以降の空 WT_STREAM も通る。

`crates/tokio-http2/src/webtransport.rs` の `WtBidiStream::recv` / `WtUniRecvStream::recv` は `fin = true` でも `Ok(Some(data))` を返す。空 FIN を受けると `Ok(Some(Vec::new()))` になり、`examples/wt_server` の `handle_bidi` / `handle_uni` はそれを通常データとして `send(data, false)` するため、example 自身が空の非 FIN WT_STREAM を送信する。

## 設計方針

- ストリームがピアに既知か (WT_STREAM / WT_MAX_STREAM_DATA / WT_STOP_SENDING のいずれかを送信済みか) と、最初の WT_STREAM かを追跡する。
- 空かつ FIN なしの送信は、その capsule が実際にストリームを開く場合だけ許可する。それ以外は `stream_state_error` を返す。
- `recv` は空 FIN を `Ok(None)` として返す、または `fin` を呼び出し側へ明示する API に変更する。
- `examples/wt_server` は空データをエコーしないように修正する。
- Sans I/O / tokio / example の各層に空 FIN・空非 FIN のテストを追加する。

## 完了条件

- 既存ストリームへの空の非 FIN WT_STREAM が `stream_state_error` になること。
- 空 FIN の受信が `Ok(None)` または `fin` を明示する契約で扱われること。
- `examples/wt_server` が空の非 FIN WT_STREAM を送信しないこと。
- 空 FIN / 空非 FIN の送受信テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
