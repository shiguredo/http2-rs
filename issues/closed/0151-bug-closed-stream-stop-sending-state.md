# 削除済みストリームへの WT_STOP_SENDING / WT_MAX_STREAM_DATA の重複・順序検証が失われる

- Created: 2026-09-12
- Completed: 2026-09-12
- Branch: feature/fix-closed-stream-stop-sending-state
- Polished: 2026-09-12

## 目的

ストリームが閉じて `WtSession` の `streams` から削除された後も、WT_STOP_SENDING の重複検証と WT_MAX_STREAM_DATA の順序検証を維持する。

draft-ietf-webtrans-http2-15 Section 6.3 は「A WT_STOP_SENDING capsule MUST NOT be sent multiple times for the same stream. ... A stream error (Section 3.4) of type WT_STREAM_STATE_ERROR MUST be sent if a second WT_STOP_SENDING capsule is received.」とし、Section 6.6 は「A WT_MAX_STREAM_DATA capsule MUST NOT be sent after a sender requests that a stream be closed with WT_STOP_SENDING. ... A stream error (Section 3.4) of type WT_STREAM_STATE_ERROR MUST be sent if a WT_MAX_STREAM_DATA capsule is received after a WT_STOP_SENDING capsule for the same stream.」とする。同 draft Section 11.3 が `WT_STREAM_STATE_ERROR` を HTTP/2 の Error Code として登録しているため、本 issue の拒否も `WtError::stream_state_error` で返す。これらの検証は `WtStream` の `stop_sending_received` フラグに依存するため、ストリーム削除後は失われる。

## 現状

`WtSession::remove_if_closed` は閉じたストリームを削除し、ID を `closed_streams` に記録する。`WtSession::handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA はストリーム不在時に受信専用 ID (ピア開始 uni) 以外を検証しないため、次のとおりすり抜ける。

- 削除済みストリームへの 2 回目の WT_STOP_SENDING がエラーにならず `WtEvent::StopSending` が再度送出される
- ピアから WT_STOP_SENDING を受信済みのストリームが削除された後、その ID への WT_MAX_STREAM_DATA が順序検証をすり抜ける

双方向ストリームが削除される経路は、送信パートと受信パートの両方が終端状態になる必要がある (`WtStream::is_closed()`)。`stop_sending_received` の再現手順は次のとおり。

- ピアから WT_STOP_SENDING を受信 (フラグ設定と WT_RESET_STREAM の自動応答で送信パートが `ResetRecvd` になる) → ピアから FIN 付き WT_STREAM を受信 → `WtSession::poll_event` で `StreamData { fin: true }` を pop (受信パートが `DataRead` になり削除)

なお順序検証の基準は「ピアから WT_STOP_SENDING を受信したか」であり、ローカルが `stop_sending` を送ったかは関係しない (RFC 9000 Section 3.3 は MAX_STREAM_DATA と STOP_SENDING のいずれもデータ受信側が送ると定めるため、受信側が検査すべきはピアの送信事実である)。

ストリームがまだ存在しない ID への WT_STOP_SENDING を受理する経路もある。`stop_sending_received` が偽のまま削除された ID への受信がこれに当たり、ストリーム不在のため受理して `WtEvent::StopSending` を送出するが、受理した事実をどこにも残さないため 2 回目も受理されてしまう。

`stop_sending_received` が偽のまま削除される ID は複数ある。ローカル開始 uni はピアから WT_STOP_SENDING を受けていなければこの状態になる (ピアからの WT_STOP_SENDING は送信専用ストリームでも正当であり、0145 の設計方針どおり受理して WT_RESET_STREAM を自動応答し、そのまま削除される)。ローカル開始 bidi でもピアから WT_STOP_SENDING を受けていない場合は同じ状態になる。ピア開始 uni は FIN 受信で削除された後に WT_STOP_SENDING を受信しても受信専用 ID として拒否されるため、この経路にはならない。

## 設計方針

- 削除後も検証できるよう、`WtStreamId` の上限付き集合 (`BoundedSet<WtStreamId>`) を **受信用に 1 つ** 新設する。`WtStream::stop_sending_received` に対応し、WT_STOP_SENDING の重複検証と WT_MAX_STREAM_DATA の順序検証の両方で参照する。ローカルが `stop_sending` を送った事実は順序検証の基準にしないため、送信用の集合は持たない (持つと、自分が WT_STOP_SENDING を送った後にピアが正当に送る WT_MAX_STREAM_DATA を誤って拒否する)
- 受信用の集合へは、ピアからの WT_STOP_SENDING を受理した時点で ID を追加する (ストリームの有無を問わない)。重複検証と順序検証がいずれも受信の事実に基づくため、集合は 1 つで足りる。`WtSession::remove_if_closed` での記録は不要になる
- `WtSession::handle_capsule` の WT_STOP_SENDING は、受信専用 ID の拒否 (0148) と未作成のローカル開始 ID の拒否 (0150) を評価した後の受理経路で、「ストリームのフラグまたは受信用の集合」を OR で評価する述語 `WtSession::stop_sending_received` で重複を検証する。該当する場合は `WtError::stream_state_error` で拒否し、イベント送出・出力生成を行わない。**本 issue は 0150 の拒否が先に評価されることを前提とする。** 0150 が未実装の状態で本 issue だけを入れると、未作成のローカル開始 ID を受理して受信用の集合へ記録することになり 0150 の非回帰を壊すため、0150 の後に実装する
- `WtSession::handle_capsule` の WT_MAX_STREAM_DATA は、同じ述語 `WtSession::stop_sending_received` で順序違反を検証する。該当する場合は `WtError::stream_state_error` で拒否し、出力生成を行わない。**判定は「ローカルが WT_STOP_SENDING を送ったか」ではなく「ピアから WT_STOP_SENDING を受信したか」を基準とする。** RFC 9000 Section 3.3 は MAX_STREAM_DATA と STOP_SENDING のいずれもデータ受信側が送ると定め、draft-ietf-webtrans-http2-15 Section 6.6 は WT_STOP_SENDING を送った側が WT_MAX_STREAM_DATA を送ることを禁じているため、受信側が検査すべきはピアの送信事実である。したがって専用の送信用集合は持たず、受信用の集合と `WtStream::stop_sending_received` で両方の検証を行う
- 既存の `closed_streams` は WT_STREAM の再作成拒否に使う記録であり、意味を混ぜないために STOP_SENDING 用の集合は新設する。上限は `STREAM_ID_RECORD_MAX_SIZE` と揃える。上限超過で追い出された ID は検証できなくなる。`BoundedSet` は最も小さい ID を追い出すため、ストリーム ID が開始主体と方向の系統をまたぐと数値順と生成順が一致せず、追い出し順が受理順と一致しない。重複判定に必要な ID が追い出されると 2 回目の WT_STOP_SENDING が受理され得るが、これは `closed_streams` と同じ既知の制限として受け入れる
- 受信専用 ID への拒否 (0148) と未作成のローカル開始 ID への拒否 (0150)、ピア開始 bidi の未知 ID の作成 (0152) は非回帰とする
- ローカル開始 ID への 1 回目の WT_STOP_SENDING は受理を維持する (0150 の完了条件)。受理と同時に受信用の集合へ ID を追加し、2 回目以降はこの記録で拒否する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- 削除済みストリームへの 2 回目の WT_STOP_SENDING が `stream_state_error` を返し、イベントが送出されず出力が生成されないこと
- ピアから WT_STOP_SENDING を受信済みで削除されたストリームへの WT_MAX_STREAM_DATA が `stream_state_error` を返し、出力が生成されないこと
- 1 回目の WT_STOP_SENDING と、ピアから WT_STOP_SENDING を受けていない ID への WT_MAX_STREAM_DATA の従来挙動が非回帰であること (削除済み ID への WT_MAX_STREAM_DATA は無視され、ストリームが再作成されない)
- ローカルが `stop_sending` を送っただけの ID への WT_MAX_STREAM_DATA について、ストリームが生存していれば受理されて送信上限が更新されること (従来は `WtStream::stop_sending_sent` を基準に誤って拒否していたため、その解消)
- 削除済みのローカル開始 bidi ID への 1 回目の WT_STOP_SENDING が従来どおり `WtEvent::StopSending` を送出し、2 回目が `stream_state_error` を返しイベントが送出されないこと (0150 の非回帰と本 issue の重複検証の両立)
- テストが追加され、`cargo test --all` が通過すること

## 解決方法

- `src/webtransport.rs` の `WtSession` に `stop_sending_received_ids: BoundedSet<WtStreamId>` を追加した。ピアからの WT_STOP_SENDING を受理した時点でストリームの有無を問わず ID を記録するため、ストリームが削除された後も重複・順序検証を維持できる。上限は `closed_streams` と共用する `STREAM_ID_RECORD_MAX_SIZE` (旧 `CLOSED_STREAMS_MAX_SIZE` をリネーム) とし、上限超過で追い出された削除済み ID は検証できないという既知の制限を定数の doc に明記した
- 判定は述語 `WtSession::stop_sending_received` (生存ストリームの `WtStream::stop_sending_received` と受信用の集合の OR) に集約した。ストリーム ID は仕様上再利用されない (RFC 9000 Section 2.1) ため、削除済み ID では集合のみが、生存中はフラグのみが該当し、OR でも誤判定しない
- `WtSession::handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA の両分岐で、受信専用 ID の拒否 (0148) と未作成のローカル開始 ID の拒否 (0150) を評価した後、暗黙のストリーム生成より前にこの述語で検証するようにした。拒否時は `WtError::stream_state_error` を返し、ストリーム生成・`WtEvent::StreamOpened` の送出・出力生成を行わないため、拒否したのに状態が残る非原子性が解消している
- WT_MAX_STREAM_DATA の順序検証の基準を `WtStream::stop_sending_sent` (ローカルが送ったか) から受信基準に改めた。WT_MAX_STREAM_DATA と WT_STOP_SENDING はいずれもデータ受信側が送る操作であり (RFC 9000 Section 3.3)、draft-ietf-webtrans-http2-15 Section 6.6 が禁じるのは WT_STOP_SENDING を送った側が WT_MAX_STREAM_DATA を送ることであるため、受信側が検査すべきはピアの送信事実である。これにより、ローカルが `stop_sending` を送った生存ストリームへピアが送る credit を誤って拒否しなくなった。送信側 API (`WtSession::send_max_stream_data` / `WtSession::grow_stream_recv_window`) の `stop_sending_sent` 検証は同方向の MUST NOT として維持している
- `tests/test_webtransport/integration.rs` に、削除済みストリームへの 2 回目の WT_STOP_SENDING の拒否、ピアから WT_STOP_SENDING を受信済みのストリームが削除された後の WT_MAX_STREAM_DATA の拒否、削除済み ID への 1 回目の受理、ローカルが `stop_sending` を送っただけのストリームへの credit の受理、削除済みのピア開始 bidi ID を WT_MAX_STREAM_DATA で再作成しないことの各テストを追加した
- `CHANGES.md` の `## develop` に `[FIX]` のエントリを追加した
