# ピア開始 bidi への先着 capsule でストリームが作成されず WT_RESET_STREAM が自動応答されない

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-peer-bidi-implicit-stream-creation
- Polished: 2026-09-12

## 目的

ピア開始 bidi の未知 ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA を受信したときに、ストリームを作成して capsule を適用する。

RFC 9000 Section 3.2 は「For bidirectional streams initiated by a peer, receipt of a MAX_STREAM_DATA or STOP_SENDING frame for the sending part of the stream also creates the receiving part. The initial state for the receiving part of a stream is "Recv".」「An endpoint opens a bidirectional stream when a MAX_STREAM_DATA or STOP_SENDING frame is received from the peer for that stream.」と定める。Section 3.5 は「An endpoint that receives a STOP_SENDING frame MUST send a RESET_STREAM frame if the stream is in the "Ready" or "Send" state.」とする。draft-ietf-webtrans-http2-15 Section 5.2 は WebTransport ストリームが QUIC のストリーム状態を mirror すると定め、同 draft Section 6.3 は「As defined in Section 3.5 of [RFC9000], the recipient of a WT_STOP_SENDING capsule sends a WT_RESET_STREAM capsule in response if the stream is in the "Ready" or "Send" state.」とする。

## 現状

`WtSession::handle_capsule` はピア開始 bidi の未知 ID への受信でストリームを作成しない。

- WT_STOP_SENDING はストリームを作成せず `WtEvent::StopSending` を送出するだけで、WT_RESET_STREAM を自動応答しない (`self.streams.get(&stream_id)` が `None` のため `should_reset` が偽になる)
- WT_MAX_STREAM_DATA はストリームを作成せず暗黙に無視する (`self.streams.get_mut(&stream_id)` が `None` のため何もしない)

この経路を固定している既存テストが 2 件ある。

- `stop_sending_unknown_peer_bidi_stream_emits_event` は「イベントは出るが出力は生成されない」を固定している
- `wt_max_stream_data_unknown_peer_bidi_stream_ignored` は「イベントも出力も生成されない」を固定している

どちらも ID 9997 を使うが、クライアント既定の `initial_max_streams_bidi` は 100 であり、`WtFlowControl::can_accept_stream` の受理上限は `100 * 4 + 1 = 401` である (`WtSession::new` が `config.initial_max_streams_bidi` を `WtFlowControl` の `max_streams_bidi_local` に渡し、`can_accept_stream` は `stream_id < max_streams * 4 + (stream_id & 0x03)` で判定する。クライアント視点のピア開始 bidi はサーバー開始 bidi なので `stream_id & 0x03` は 1)。9997 はこの上限を超えるため、ストリーム作成を導入した時点で両テストは `flow_control_error` になり、期待値の書き換えが必要になる。

## 設計方針

- 作成の対象は **ピア開始 bidi の未知 ID に限る**。WT_STOP_SENDING / WT_MAX_STREAM_DATA の分岐で、受信専用 ID の拒否 (0148、実装済み) を評価した後、作成の可否を「ピア開始 bidi の未知 ID か」と「その ID が `closed_streams` に記録済みか」の 2 点で判定する。記録済みであれば拒否し、記録がなければストリームを作成する。`closed_streams` に記録済みの ID へストリームを作り直すと `WtEvent::StreamOpened` が再送出され、クローズ済みストリームを再作成しない方針 (0128) に反するため、この拒否は必要である
- この `closed_streams` の拒否はピア開始 bidi に限定する。`WtSession::remove_if_closed` は開始主体と方向を問わず ID を記録するため、限定しないと削除済みのローカル開始 bidi ID への WT_STOP_SENDING まで拒否してしまう。0150 の完了条件は「閉じて削除済みのローカル開始 ID への WT_STOP_SENDING が `WtEvent::StopSending` を送出し、WT_MAX_STREAM_DATA が暗黙に無視されること (受理の維持)」、0151 の完了条件は「削除済みのローカル開始 bidi ID への 1 回目の WT_STOP_SENDING が従来どおり `WtEvent::StopSending` を送出すること」であり、いずれも削除済みローカル開始 ID の受理維持を要求する
- 未作成のローカル開始 ID の拒否 (0150) は本 issue では実装しない。作成の対象がピア開始 bidi に限られるため 0150 への依存はなく、0150 の拒否は 0150 側で追加する
- ストリーム作成の処理は `WtSession::handle_stream_data` の新規ストリーム作成と同じ内容にする。`WtFlowControl::can_accept_stream` による受信ストリーム数上限の検証、`WtStream::new` による生成と `streams` への挿入、`WtEvent::StreamOpened` の送出を行う。`handle_stream_data` 側と重複する処理は共通のヘルパーに切り出し、両経路で同じ内容を使う。ただし `handle_stream_data` に固有の処理 (空 capsule チェック、ローカル開始 ID の拒否、WT_STREAM 向けの `closed_streams` 拒否メッセージ) は共通化しない
- 初期値はピア開始 bidi の既存経路と揃える。`send_max` は `peer_config.initial_max_stream_data_bidi_local`、`recv_max` は `config.initial_max_stream_data_bidi_remote` とする (draft-ietf-webtrans-http2-15 Section 11.2)
- WT_STOP_SENDING では作成したストリームに `WtEvent::StopSending` を送出し、`Ready` または `Send` 状態であれば WT_RESET_STREAM を自動応答する。既存の自動応答と同じく `WtSession::reset_stream` を呼び、その戻り値は破棄する。エラーコードは capsule のデコード時に `0xffffffff` 以下が検証済みで、ストリームは直前に作成済みのため `reset_stream` が失敗する経路はない
- WT_MAX_STREAM_DATA では作成したストリームの送信上限を `WtStream::update_send_max` で更新する
- 受信専用 ID (ピア開始 uni) への拒否 (0148) は実装済みであり、非回帰として変更しない。未作成のローカル開始 ID の拒否 (0150) は本 issue では実装しない (前掲)
- 受信ストリーム数上限に達している場合は `flow_control_error` を返し、ストリームを作成しない
- 上記 2 件の既存テストを、受理上限内の ID を使い新しい挙動 (ストリーム作成・`WtEvent::StreamOpened`・WT_RESET_STREAM の自動応答・送信上限の更新) を検証するテストに書き換える。`wt_max_stream_data_unknown_peer_bidi_stream_ignored` は「無視する」という名前と doc コメントが新しい挙動と逆になるため改名する。0148 が `stop_sending_unknown_stream_emits_event` を `stop_sending_unknown_peer_bidi_stream_emits_event` へ改名した前例に倣う
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- 未知のピア開始 bidi ID への WT_STOP_SENDING 受信でストリームが作成され、`WtEvent::StreamOpened` と `WtEvent::StopSending` が送出され、WT_RESET_STREAM の自動応答が出力されること
- 作成されたストリームの送信状態が `ResetRecvd` になり、自動応答の capsule が `stream_id` と `error_code` (WT_STOP_SENDING の値をコピー) と `reliable_size` (送信済みバイト数) を持つこと
- 未知のピア開始 bidi ID への WT_MAX_STREAM_DATA 受信でストリームが作成され、`WtEvent::StreamOpened` が送出され、送信上限が `maximum` に更新されること
- 削除済みで `closed_streams` に記録済みのピア開始 bidi ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA は拒否され、ストリームが再作成されず `WtEvent::StreamOpened` が送出されないこと
- 削除済みのローカル開始 bidi ID への WT_STOP_SENDING は `WtEvent::StopSending` を送出し、WT_MAX_STREAM_DATA は暗黙に無視されること (0150 / 0151 の完了条件の非回帰)
- 受信ストリーム数上限に達している場合は `flow_control_error` を返し、ストリームが作成されないこと
- 受信専用 ID (ピア開始 uni) への拒否 (0148) が非回帰であること。未作成のローカル開始 ID への拒否は本 issue では実装しない (0150 で対応する)
- 既存の 2 テスト (`stop_sending_unknown_peer_bidi_stream_emits_event` / `wt_max_stream_data_unknown_peer_bidi_stream_ignored`) が新しい挙動を検証する内容に更新され、`wt_max_stream_data_unknown_peer_bidi_stream_ignored` は挙動を表す名前へ改名されること
- テストが追加され、`cargo test --all` が通過すること
