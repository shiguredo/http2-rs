# 未作成のローカル開始 ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA が拒否されない

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-uncreated-local-stream-id-validation
- Polished: 2026-09-12

## 目的

未作成のローカル開始ストリーム ID 宛の WT_STOP_SENDING / WT_MAX_STREAM_DATA を WT_STREAM_STATE_ERROR として拒否する。

RFC 9000 Section 19.5 は「Receiving a STOP_SENDING frame for a locally initiated stream that has not yet been created MUST be treated as a connection error of type STREAM_STATE_ERROR.」、Section 19.10 も同じ文型で MAX_STREAM_DATA について定める。draft-ietf-webtrans-http2-15 Section 5.2 は WebTransport ストリームの識別子と状態が QUIC ストリームのそれらを mirror すると定め、同 draft Section 3.4 は capsule の状態違反をストリームエラー (`RST_STREAM`) として伝えるとし、同 draft Section 11.3 が `WT_STREAM_STATE_ERROR` (「Unexpected WebTransport stream-related capsule received」) を HTTP/2 の Error Code として登録している。したがって QUIC の connection error は WebTransport 上ではストリームエラーとして通知する。

0148 は受信専用 ID (ピア開始 uni) を静的判定で拒否したが、ローカル開始 ID は対象外だった。RFC 9000 Section 19.5 / Section 19.10 の MUST は 2 文構成であり、0148 が 2 文目 (受信専用)、本 issue が 1 文目 (未作成のローカル開始) を担当する。

## 現状

`WtSession::handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA は受信専用 ID (ピア開始 uni) のみを拒否し、未作成のローカル開始 ID は次のとおりすり抜ける。

- 未作成のローカル開始 ID への WT_STOP_SENDING はエラーにならず `WtEvent::StopSending` が送出される
- 未作成のローカル開始 ID への WT_MAX_STREAM_DATA は暗黙に無視される

ローカル開始 ID は `next_bidi_stream_id` / `next_uni_stream_id` が示す採番済み範囲より先かどうかで「未作成」を判定できる。両カウンタは `WtSession::new` で `stream::stream_id::first` により初期化され (クライアントは bidi 0 / uni 2、サーバーは bidi 1 / uni 3)、`WtSession::open_stream` の 1 箇所だけで `stream::stream_id::next` (+4) され、その直後に `streams` へ挿入される。他に更新する箇所はなく、`WtSession::handle_stream_data` はローカル開始 ID のストリームを作らないため、`id < next_bidi_stream_id` が「作成済み」、`id >= next_bidi_stream_id` が「未作成」と厳密に一致する (uni も同様)。RFC 9000 Section 2.1 の「A stream ID that is used out of order results in all streams of that type with lower-numbered stream IDs also being opened.」とも整合する。

## 設計方針

- ロールと ID からローカル開始と判定できる ID のうち、未作成のものを `WtError::stream_state_error` で拒否する。ローカル開始の判定には 0148 で追加した `WtSession::is_peer_initiated` の否定を使い、新たな判定を書き起こさない。未作成の判定は `id >= next_bidi_stream_id` (双方向) または `id >= next_uni_stream_id` (単方向) とする
- この拒否条件は `WtSession::is_receive_only_id` の判定と排他である (受信専用 ID はピア開始 uni、ローカル開始 ID はピア開始でない)。両者の記述順は挙動に影響しない
- 拒否時はイベント送出・出力生成を行わない
- 既に閉じて削除済みのローカル開始 ID (採番済み範囲内) への受信は受理を維持する。RFC 9000 Section 19.5 / Section 19.10 の MUST は "has not yet been created" の場合のみを対象とし、削除済み ID は作成済みのため対象外である。受理の中身は現行どおり、WT_STOP_SENDING が `WtEvent::StopSending` を送出し (ストリーム不在のため自動 WT_RESET_STREAM は応答しない)、WT_MAX_STREAM_DATA は暗黙に無視する。1 回目の WT_STOP_SENDING は受理するが、draft-ietf-webtrans-http2-15 Section 6.3 の重複検証と Section 6.6 の順序検証は 0151 の範囲とし、本 issue では扱わない
- 受信専用 ID (ピア開始 uni) への拒否は非回帰とする
- ピア開始 bidi の未知 ID への受信は本 issue では変更しない。現行は WT_STOP_SENDING がイベント送出のみ、WT_MAX_STREAM_DATA が暗黙に無視だが、この経路の扱いは 0152 の完了条件に従う
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- 未作成のローカル開始 bidi / uni ID への WT_STOP_SENDING が `stream_state_error` を返し、イベントが送出されず出力が生成されないこと
- 未作成のローカル開始 bidi / uni ID への WT_MAX_STREAM_DATA が `stream_state_error` を返し、イベントが送出されず出力が生成されないこと
- 受信専用 ID (ピア開始 uni) への拒否が非回帰であること
- 閉じて削除済みのローカル開始 ID への WT_STOP_SENDING が `WtEvent::StopSending` を送出し、WT_MAX_STREAM_DATA が暗黙に無視されること (受理の維持)
- 開設済みのローカル開始 ID への受信が従来どおり動作すること
- テストが追加され、`cargo test --all` が通過すること
