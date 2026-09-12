# ピア開始 bidi への先着 capsule でストリームが作成されず WT_RESET_STREAM が自動応答されない

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-peer-bidi-implicit-stream-creation
- Polished: {YYYY-MM-DD}

## 目的

ピア開始 bidi の未知 ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA を受信したときに、ストリームを作成して capsule を適用する。RFC 9000 Section 3.2 は「An endpoint opens a bidirectional stream when a MAX_STREAM_DATA or STOP_SENDING frame is received from the peer for that stream」とし、Section 19.5 は Ready / Send 状態で STOP_SENDING を受信したエンドポイントに RESET_STREAM の送信を MUST とする。draft-ietf-webtrans-http2-15 Section 5.2 は WebTransport ストリームが QUIC のストリーム状態を mirror すると定めている。

## 現状

`WtSession::handle_capsule` はピア開始 bidi の未知 ID への受信でストリームを作成しない。

- WT_STOP_SENDING はストリームを作成せず `WtEvent::StopSending` を送出するだけで、WT_RESET_STREAM を自動応答しない
- WT_MAX_STREAM_DATA はストリームを作成せず暗黙に無視する

## 設計方針

- ピア開始 bidi の未知 ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA 受信時にストリームを作成し、受信ストリーム数上限 (`can_accept_stream`) の検証とイベント送出を行う
- WT_STOP_SENDING では Ready / Send 状態のストリームに WT_RESET_STREAM を自動応答する (既存の自動応答と同じ検証・出力経路を使う)
- 受信専用 ID (ピア開始 uni) への拒否 (0148) と未作成のローカル開始 ID の扱いは本 issue のスコープ外とし、変更しない
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- 未知のピア開始 bidi ID への WT_STOP_SENDING 受信でストリームが作成され、`WtEvent::StopSending` の送出と WT_RESET_STREAM の自動応答が行われること
- 未知のピア開始 bidi ID への WT_MAX_STREAM_DATA 受信でストリームが作成され、送信上限が更新されること
- 受信ストリーム数上限に達している場合は `flow_control_error` になること
- テストが追加され、`cargo test --all` が通過すること
