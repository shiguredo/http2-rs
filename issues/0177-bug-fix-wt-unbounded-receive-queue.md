# WebTransport 受信データの無制限キューイングと自動 credit を解消する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-unbounded-receive-queue
- Polished: {YYYY-MM-DD}

## 目的

`tokio-http2` の WebTransport driver は bidi / uni / DATAGRAM / 各ストリームの受信に `mpsc::unbounded_channel()` を使い、アプリが受信しない場合でもデータを無制限に蓄積する。さらに受信量だけを基準に `WT_MAX_DATA` / `WT_MAX_STREAM_DATA` と HTTP/2 `WINDOW_UPDATE` を自動送信するため、遅いアプリでもピアは送り続けられる。リモートからのメモリ DoS を防ぐため、受信バッファと credit の設計を見直す。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState` は `bidi_tx` / `uni_tx` / `datagram_tx` と各ストリームの `stream_channels` を unbounded channel で生成する。`dispatch_wt_event` は `StreamData` をそのまま channel へ送り、`maybe_grow_session_window` / `maybe_grow_stream_window` はアプリの消費ではなく `WtSession` の受信カウントだけを見てウィンドウを拡張する。

DATAGRAM は WebTransport のフロー制御対象外であり、`datagram_tx` も unbounded である。draft-ietf-webtrans-http2-16 Section 4.4 は消費に応じた credit 返却を SHOULD、Section 6.11 は DATAGRAM の破棄を許容している。RFC 9297 Section 3.2 も capsule 全量の蓄積を避けるよう求めている。

## 設計方針

- 受信チャネルを bounded にし、アプリが消費した分だけ credit を返す。
- DATAGRAM は容量上限を設け、超過時に破棄する。破棄数を観測できる API を検討する。
- HTTP/2 の WINDOW_UPDATE と WT_MAX_DATA / WT_MAX_STREAM_DATA の送信を、アプリ消費の完了と結びつける。
- 遅い消費者を模した統合テストで、メモリ使用量とピアの送信継続が上限内に収まることを検証する。

## 完了条件

- アプリが受信しない状態でピアが送り続けても、driver のメモリ使用量が設定した上限を超えないこと。
- DATAGRAM の受信バッファ上限と破棄方針が実装され、doc に明記されていること。
- 遅い消費者を模した回帰テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
