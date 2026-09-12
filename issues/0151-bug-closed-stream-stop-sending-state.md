# 削除済みストリームへの WT_STOP_SENDING / WT_MAX_STREAM_DATA の重複・順序検証が失われる

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-closed-stream-stop-sending-state
- Polished: {YYYY-MM-DD}

## 目的

ストリームが閉じて `WtSession` の `streams` から削除された後も、WT_STOP_SENDING の重複検証と WT_MAX_STREAM_DATA の順序検証を維持する。draft-ietf-webtrans-http2-15 Section 6.3 は 2 回目の WT_STOP_SENDING 受信に WT_STREAM_STATE_ERROR を MUST とし、Section 6.6 は WT_STOP_SENDING 送信後の WT_MAX_STREAM_DATA 受信に WT_STREAM_STATE_ERROR を MUST としている。これらの検証は `WtStream` の `stop_sending_received` / `stop_sending_sent` フラグに依存するため、ストリーム削除後は失われる。

## 現状

`WtSession::remove_if_closed` は閉じたストリームを削除し、ID を `closed_streams` に記録する。`handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA はストリーム不在時に受信専用 ID (ピア開始 uni) 以外を検証しないため、次のとおりすり抜ける。

- 削除済みストリームへの 2 回目の WT_STOP_SENDING がエラーにならず `WtEvent::StopSending` が再度送出される
- WT_STOP_SENDING を送信済みのストリームが削除された後、その ID への WT_MAX_STREAM_DATA が暗黙に無視される

## 設計方針

- 削除時に `stop_sending_received` / `stop_sending_sent` の検証に必要な状態を `closed_streams` などの永続的な記録へ残す方法を検討し、ストリーム不在時も両 MUST を検証できるようにする。状態の保持方法 (用途別の ID 集合を分ける・上限付きの記録にする等) は実装時に決める
- 検証に該当する場合は `WtError::stream_state_error` で拒否し、イベント送出・出力生成を行わない
- 受信専用 ID への拒否 (0148) とその他の非回帰は維持する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- 削除済みストリームへの 2 回目の WT_STOP_SENDING が `stream_state_error` を返し、イベントが送出されないこと
- WT_STOP_SENDING 送信済みで削除されたストリームへの WT_MAX_STREAM_DATA が `stream_state_error` を返すこと
- 1 回目の WT_STOP_SENDING と、未送信ストリームへの WT_MAX_STREAM_DATA の従来挙動が非回帰であること
- テストが追加され、`cargo test --all` が通過すること
