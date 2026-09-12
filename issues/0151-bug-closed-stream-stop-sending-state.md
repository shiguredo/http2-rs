# 削除済みストリームへの WT_STOP_SENDING / WT_MAX_STREAM_DATA の重複・順序検証が失われる

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-closed-stream-stop-sending-state
- Polished: 2026-09-12

## 目的

ストリームが閉じて `WtSession` の `streams` から削除された後も、WT_STOP_SENDING の重複検証と WT_MAX_STREAM_DATA の順序検証を維持する。

draft-ietf-webtrans-http2-15 Section 6.3 は「A WT_STOP_SENDING capsule MUST NOT be sent multiple times for the same stream. ... A stream error (Section 3.4) of type WT_STREAM_STATE_ERROR MUST be sent if a second WT_STOP_SENDING capsule is received.」とし、Section 6.6 は「A WT_MAX_STREAM_DATA capsule MUST NOT be sent after a sender requests that a stream be closed with WT_STOP_SENDING. ... A stream error (Section 3.4) of type WT_STREAM_STATE_ERROR MUST be sent if a WT_MAX_STREAM_DATA capsule is received after a WT_STOP_SENDING capsule for the same stream.」とする。同 draft Section 11.3 が `WT_STREAM_STATE_ERROR` を HTTP/2 の Error Code として登録しているため、本 issue の拒否も `WtError::stream_state_error` で返す。これらの検証は `WtStream` の `stop_sending_received` / `stop_sending_sent` フラグに依存するため、ストリーム削除後は失われる。

## 現状

`WtSession::remove_if_closed` は閉じたストリームを削除し、ID を `closed_streams` に記録する。`WtSession::handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA はストリーム不在時に受信専用 ID (ピア開始 uni) 以外を検証しないため、次のとおりすり抜ける。

- 削除済みストリームへの 2 回目の WT_STOP_SENDING がエラーにならず `WtEvent::StopSending` が再度送出される
- WT_STOP_SENDING を送信済みのストリームが削除された後、その ID への WT_MAX_STREAM_DATA が暗黙に無視される

双方向ストリームが削除される経路は、送信パートと受信パートの両方が終端状態になる必要がある (`WtStream::is_closed()`)。そのため各検証の再現手順は次のとおり。

- `stop_sending_received`: ピアから WT_STOP_SENDING を受信 (フラグ設定と WT_RESET_STREAM の自動応答で送信パートが `ResetRecvd` になる) → ピアから FIN 付き WT_STREAM を受信 → `WtSession::poll_event` で `StreamData { fin: true }` を pop (受信パートが `DataRead` になり削除)
- `stop_sending_sent`: ローカルから `stop_sending` を送信 (フラグ設定のみで送信状態は変わらない) → 送信パートを終端させる (`reset_stream` で `ResetRecvd`、または `send_data` の FIN で `DataRecvd`) → ピアから FIN 付き WT_STREAM を受信して `poll_event` で削除

ストリームがまだ存在しない ID への WT_STOP_SENDING を受理する経路もある。両フラグが偽のまま削除された ID への受信がこれに当たり、ストリーム不在のため受理して `WtEvent::StopSending` を送出するが、受理した事実をどこにも残さないため 2 回目も受理されてしまう。

両フラグが偽のまま削除される ID は複数ある。ローカル開始 uni は `WtSession::stop_sending` が `WtStream::has_recv_part()` の検証で拒否するため送信側のフラグは立ち得ず、ピアから WT_STOP_SENDING を受けていなければ受信側のフラグも立たない (ピアからの WT_STOP_SENDING は送信専用ストリームでも正当であり、0145 の設計方針どおり受理して WT_RESET_STREAM を自動応答し、そのまま削除される)。ローカル開始 bidi でも、`stop_sending` を送らずピアから WT_STOP_SENDING も受けていない場合は同じ状態になる。ピア開始 uni は FIN 受信で削除された後に WT_STOP_SENDING を受信しても受信専用 ID として拒否されるため、この経路にはならない。

## 設計方針

- 削除後も検証できるよう、`WtStreamId` の上限付き集合 (`BoundedSet<WtStreamId>`) を **受信用と送信用の 2 つ** 新設する。受信用は `stop_sending_received`、送信用は `stop_sending_sent` に対応する。1 つの集合に両フラグを混ぜると、`stop_sending_sent` だけが真で削除された ID へのピアからの 1 回目の WT_STOP_SENDING が重複扱いになり 0150 の非回帰を壊すため、必ず分ける。`stop_sending_sent` だけが真で削除される ID は、ローカル開始 bidi で `stop_sending` を送信した後に `reset_stream` または FIN 送信で送信パートを終端させ、ピア FIN の `poll_event` で削除された場合に作れる
- 送信用の集合へはローカルが WT_STOP_SENDING を送信した時点 (`WtSession::stop_sending` が成功した時点) で ID を追加する。受信用の集合へは、ストリームが存在する場合はピアからの WT_STOP_SENDING を受理した時点で、ストリーム不在の場合は同じく受理した時点で ID を追加する。`WtSession::remove_if_closed` での記録は不要になる
- `WtSession::handle_capsule` の WT_STOP_SENDING は、受信専用 ID の拒否 (0148) と未作成のローカル開始 ID の拒否 (0150) を評価した後の受理経路で、ストリームが存在すれば従来どおり `WtStream::stop_sending_received()` を検証し、存在しなければ受信用の集合を検証する。いずれかに該当する場合は `WtError::stream_state_error` で拒否し、イベント送出・出力生成を行わない。**本 issue は 0150 の拒否が先に評価されることを前提とする。** 0150 が未実装の状態で本 issue だけを入れると、未作成のローカル開始 ID を受理して受信用の集合へ記録することになり 0150 の非回帰を壊すため、0150 の後に実装する
- `WtSession::handle_capsule` の WT_MAX_STREAM_DATA は、ストリームが存在すれば従来どおり `WtStream::stop_sending_sent()` を検証し、存在しなければ送信用の集合を検証する。該当する場合は `WtError::stream_state_error` で拒否し、出力生成を行わない
- 既存の `closed_streams` は WT_STREAM の再作成拒否に使う記録であり、意味を混ぜないために STOP_SENDING 用の集合は新設する。上限は `CLOSED_STREAMS_MAX_SIZE` と揃える。上限超過で追い出された ID は検証できなくなる。`BoundedSet` は最も小さい ID を追い出すため、ストリーム ID が開始主体と方向の系統をまたぐと数値順と生成順が一致せず、追い出し順が受理順と一致しない。重複判定に必要な ID が追い出されると 2 回目の WT_STOP_SENDING が受理され得るが、これは `closed_streams` と同じ既知の制限として受け入れる
- 受信専用 ID への拒否 (0148) と未作成のローカル開始 ID への拒否 (0150)、ピア開始 bidi の未知 ID の作成 (0152) は非回帰とする
- ローカル開始 ID への 1 回目の WT_STOP_SENDING は受理を維持する (0150 の完了条件)。受理と同時に受信用の集合へ ID を追加し、2 回目以降はこの記録で拒否する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- 削除済みストリームへの 2 回目の WT_STOP_SENDING が `stream_state_error` を返し、イベントが送出されず出力が生成されないこと
- WT_STOP_SENDING 送信済みで削除されたストリームへの WT_MAX_STREAM_DATA が `stream_state_error` を返し、出力が生成されないこと
- 1 回目の WT_STOP_SENDING と、送信用の集合に記録がない ID への WT_MAX_STREAM_DATA の従来挙動が非回帰であること
- 削除済みのローカル開始 bidi ID への 1 回目の WT_STOP_SENDING が従来どおり `WtEvent::StopSending` を送出し、2 回目が `stream_state_error` を返しイベントが送出されないこと (0150 の非回帰と本 issue の重複検証の両立)
- テストが追加され、`cargo test --all` が通過すること
