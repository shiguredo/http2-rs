# `WtSession::reset_stream` が未送信のローカルストリームで相手側エラーになる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-reset-unopened-stream
- Polished: {YYYY-MM-DD}

## 目的

ローカルでストリームを開いた直後、WT_STREAM を一度も送らずに `WtSession::reset_stream` すると、ピアがまだ知らないストリーム ID へ WT_RESET_STREAM が送られる。受信側は未知ストリームを `WT_STREAM_STATE_ERROR` にするため、同じライブラリ同士でもセッションが切れる。closed 0060 が「未知ストリームへの WT_RESET_STREAM はエラー」と決定済みであるため、受信側の挙動は変えず、送信側で相手が知らないストリームを reset しないように修正する。

## 現状

`src/webtransport.rs` の `WtSession::reset_stream` は、ローカル開始ストリームが `Ready` 状態でも WT_RESET_STREAM を送る。受信側の `handle_capsule` の `WtResetStream` 分岐は `self.streams.get_mut(&stream_id)` が失敗すると `WT_STREAM_STATE_ERROR` を返す。closed 0060 の決定により、受信側で未知ストリームを暗黙生成する変更は行わない。

再現手順:

1. client が `open_bidi_stream` でストリーム 0 を開く。
2. WT_STREAM を送らずに `reset_stream(0, 1)` を呼ぶ。
3. server がその WT_RESET_STREAM を `process()` すると `WT_RESET_STREAM received for unknown stream` になる。

## 設計方針

- 送信側で、未送信のローカル開始ストリームを reset する前に、そのストリームを開くための空 WT_STREAM (FIN なし) を送る。空 WT_STREAM の送受信契約 (別 issue) と整合させる。
- または、ピアがストリームを知るまで `reset_stream` を `stream_state_error` で拒否する。どちらを採るかを決め、doc に明記する。
- 受信側の未知ストリーム拒否は closed 0060 の決定を維持し、本 issue では変更しない。
- client / server、bidi / uni の各組み合わせで、open 直後の reset を検証するテストを追加する。

## 完了条件

- open 直後の `reset_stream` が、同一ライブラリの受信側で `WT_STREAM_STATE_ERROR` にならないこと。
- 送信側の「ピアがストリームを知っているか」の判定と、その契約がコードコメントと doc に明記されていること。
- 受信側の未知ストリーム拒否 (closed 0060) が変更されていないこと。
- client / server、bidi / uni のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
