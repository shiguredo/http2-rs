# 送信専用ストリームへの受信系操作が方向検証されない

- Created: 2026-09-12
- Completed: 2026-09-12
- Branch: feature/fix-send-only-recv-operations-validation
- Polished: 2026-09-12

## 目的

ローカル開始 uni ストリーム (送信専用) に対して、受信側を前提とする操作が方向検証されず、仕様違反の capsule を送信する問題を修正する。RFC 9000 Section 3.3 は STOP_SENDING / MAX_STREAM_DATA を受信側が送るものとし、送信専用ストリーム (ローカルが送信側) から送ることは Section 19.5 / Section 19.10 に照らして不正である。

## 現状

`WtStream` には 0145 で `has_recv_part()` が追加されたが、送信専用ストリームへの受信側操作にガードがなく、次の 3 API がそのまま通過する。

- `WtSession::stop_sending` が送信専用ストリームへ WT_STOP_SENDING を送信できる
- `WtSession::send_max_stream_data` が送信専用ストリームへ WT_MAX_STREAM_DATA を送信できる
- `WtSession::grow_stream_recv_window` が送信専用ストリームの recv_max を更新し WT_MAX_STREAM_DATA を送信できる

これらの capsule を受け取ったピアは RFC 9000 Section 19.5 / Section 19.10 の MUST に従い STREAM_STATE_ERROR とする (draft-ietf-webtrans-http2-15 Section 6.3 / Section 6.6 の WT_STREAM_STATE_ERROR)。

## 設計方針

- `stop_sending` / `send_max_stream_data` / `grow_stream_recv_window` で `has_recv_part()` を検証し、送信専用ストリームへの操作を `WtError::stream_state_error` で拒否する。拒否時は `recv_max` の更新や capsule のエンコードを行わない
- 受信専用ストリーム (ピア開始 uni) と双方向ストリームへのこれらの操作は従来どおり受理する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- ローカル開始 uni への `stop_sending` が `stream_state_error` を返し、出力が生成されないこと
- ローカル開始 uni への `send_max_stream_data` が `stream_state_error` を返し、出力が生成されないこと
- ローカル開始 uni への `grow_stream_recv_window` が `stream_state_error` を返し、`recv_available` が変化しないこと
- 受信専用ストリームと双方向ストリームへの同操作は従来どおり動作すること
- テストが追加され、`cargo test --all` が通過すること

## 解決方法

- `src/webtransport.rs` の `WtSession::stop_sending` / `send_max_stream_data` / `grow_stream_recv_window` に `WtStream::has_recv_part()` の検証を追加し、送信専用のローカル開始 uni ストリームへの受信系操作を `WtError::stream_state_error` で拒否するようにした。`stop_sending` は送信済みフラグ設定と capsule エンコードの前、`grow_stream_recv_window` は `recv_max` 更新の前に拒否し、部分的な状態変更を残さない (draft-ietf-webtrans-http2-15 Section 5.2 / Section 6.3 / Section 6.6 / RFC 9000 Section 3.3 / Section 19.5 / Section 19.10)
- 受信専用 (ピア開始 uni) と双方向ストリームへの `stop_sending` / `send_max_stream_data` / `grow_stream_recv_window` は従来どおり受理する
- `tests/test_webtransport/integration.rs` に拒否 3 テスト (エラー種別 / 出力なし / フラグ・受信ウィンドウ不変 / エンコーダー残留なし) と、双方向ストリームの受理を検証する非回帰テスト (カプセルの `stream_id` / `maximum` 検証を含む) を追加した
- `CHANGES.md` の `## develop` に `[FIX]` のエントリを追加した
