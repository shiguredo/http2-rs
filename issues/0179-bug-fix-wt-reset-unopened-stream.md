# `WtSession::reset_stream` がピア未通知のローカルストリームで相手側エラーになる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-reset-unopened-stream
- Polished: 2026-11-17

## 目的

ローカルでストリームを開いた直後、WT_STREAM を一度も送らずに `WtSession::reset_stream` すると、ピアがまだ知らないストリーム ID へ WT_RESET_STREAM が送られる。受信側は未知ストリームを `WT_STREAM_STATE_ERROR` にするため、同じライブラリ同士でもセッションが切れる。closed 0060 が「未知ストリームへの WT_RESET_STREAM はエラー」と決定済みであるため、受信側の挙動は変えず、送信側でピアが知らないストリームへ WT_RESET_STREAM を単独で送らないように修正する。ピア未通知のローカル開始ストリームでは、そのストリームを開くための空 WT_STREAM を先に送ってから WT_RESET_STREAM を送る。

## 現状

`src/webtransport.rs` の `WtSession::reset_stream` は、ローカル開始ストリームが `Ready` 状態でも WT_RESET_STREAM を送る。ピアがストリームを知っているかは判定していない。受信側の `handle_capsule` の `WtResetStream` 分岐は `self.streams.get_mut(&stream_id)` が失敗すると `WtError::stream_state_error` を返す。closed 0060 (`issues/closed/0060-bug-fix-wt-reset-stream-unregistered-error.md`) の決定により、受信側で未知ストリームを暗黙生成する変更は行わない。tokio-http2 の driver は `WtSession::process` のエラーを `abort_session_with_wt_error` で CONNECT ストリームの RST_STREAM に変換するため、セッションが終了する。現行実装にはピア既知追跡がなく、`issues/0178-bug-fix-wt-empty-stream-contract.md` で追加予定である。

再現手順:

1. client が `open_bidi_stream` でストリーム 0 を開く。
2. WT_STREAM を送らずに `reset_stream(0, 1)` を呼ぶ。このとき client の出力は WT_RESET_STREAM のみである。
3. server がまだストリーム 0 を受信していない状態で、client の出力を `feed` して `process()` すると `WT_RESET_STREAM received for unknown stream` になる。

## 設計方針

- 採用する方針: ピアがストリームを知らない場合に限り、`reset_stream` はそのストリームを開くための空 WT_STREAM (`Capsule::WtStream { stream_id, data: Vec::new(), fin: false }`) を先に送り、その後に WT_RESET_STREAM を送る。ピアが既にストリームを知っている場合は WT_RESET_STREAM のみを送る。
- ピアがストリームを知っているかの判定は `issues/0178-bug-fix-wt-empty-stream-contract.md` が導入する追跡を使う。0178 の定義では、ピア開始ストリーム、自らが WT_STREAM / WT_MAX_STREAM_DATA / WT_STOP_SENDING を送信済みのストリーム、および同一型・同方向の上位 ID への送信でピアが下位 ID を暗黙に開いたストリームが「ピアに既知」である (RFC 9000 Section 2.1 / Section 3.2 / Section 3.3 / draft-ietf-webtrans-http2-16 Section 6.3 / Section 6.6 / Section 6.7)。本 issue ではこの追跡を二重実装しない。0178 が未実装の場合は 0178 を先に実装し、その追跡を利用する。
- 空 WT_STREAM をピア既知のストリームへ送ると、ストリームを開始も終了もしない空 capsule になり、draft-ietf-webtrans-http2-16 Section 6.4 の MUST NOT に反する（受信側は MAY でセッションエラーにできる）。したがって空 WT_STREAM はピア未通知のときだけ送る。
- 空 WT_STREAM は 0 バイトであり `WtStream::send_offset` を進めないため、WT_RESET_STREAM の Reliable Size は 0 のまま受信側の `recv_offset` と一致する (draft-ietf-webtrans-http2-16 Section 6.2)。空 WT_STREAM を送った場合はピア既知追跡を更新し、以後の reset で空 WT_STREAM を重複送信しない。
- 既存の検証順（`error_code` の上限、ストリームの存在、`can_send`、`has_send_part`）とセッション状態の扱いを変えない。空 WT_STREAM は `WtSession::send_stream_data` を経由せず直接エンコードし、`reset_stream` の既存契約（セッション状態を検査しない）を変えない。空 WT_STREAM は WT_RESET_STREAM より先に `output_buffer` へ積み、HTTP/2 の順序配送でピアが先にストリームを認識できるようにする。
- 代替案（ピアがストリームを知るまで `reset_stream` を `stream_state_error` で拒否する）は採らない。`tests/test_webtransport/root.rs` の `test_duplicate_operations_are_errors` が open 直後の `reset_stream` 成功を前提にしており、0178 の完了条件も「0179 が reset 前に空 WT_STREAM を送る方針を採る場合も、この契約に依存する」と明記しているため。
- 受信側の未知ストリーム拒否は closed 0060 の決定を維持し、本 issue では変更しない。ピアは空 WT_STREAM を新規ストリームとして受理し、`WtEvent::StreamOpened` と空の `WtEvent::StreamData` を送出した後に `WtEvent::StreamReset` を送出する。これは draft-ietf-webtrans-http2-16 Section 6.4 がストリームを開く空 WT_STREAM を許可しているためである。
- 変更対象は `src/webtransport.rs` の `WtSession::reset_stream`、`tests/test_webtransport/integration.rs`、`skills/shiguredo-http2/SKILL.md` の `WtSession` 表。0178 のピア既知追跡は 0178 側の変更を利用し、本 issue では重複実装しない。

## 完了条件

- open 直後の `reset_stream` が、同一ライブラリの受信側で `WT_STREAM_STATE_ERROR` にならないこと。client / server、bidi / uni の各組み合わせで、出力が空 WT_STREAM 1 件と WT_RESET_STREAM 1 件の順になり、受信側が `WtEvent::StreamOpened`、空の `WtEvent::StreamData` (`fin: false`)、`WtEvent::StreamReset` (`reliable_size: 0`) を順に観測し、`process()` がエラーを返さず、セッション状態が Active のままであることをテストで確認する。
- ピアが既にストリームを知っている場合（WT_STREAM 送信済み、WT_MAX_STREAM_DATA / WT_STOP_SENDING 送信済み、上位 ID の送信で暗黙に開いた場合、ピア開始ストリーム）は、`reset_stream` が空 WT_STREAM を送らず WT_RESET_STREAM のみを出力すること。既存の `wt_reset_stream_send_uses_send_offset` と `peer_bidi_stream_send_operations_accepted` が引き続き通ること。
- 0178 のピア既知追跡のテストが通り、既知判定が上記の各ケースをカバーしていること。
- 送信側の「ピアがストリームを知っているか」の判定と契約が、`WtSession::reset_stream` の rustdoc とコードコメント、および `skills/shiguredo-http2/SKILL.md` の `WtSession` 表の `reset_stream` 行に明記されていること。
- 受信側の未知ストリーム拒否 (closed 0060) が変更されていないこと。`src/webtransport.rs` の `handle_capsule` の `WtResetStream` 分岐を変更せず、既存の `wt_reset_stream_unknown_stream_id_errors` が通ること。
- `tests/test_webtransport/integration.rs` に上記のテストが追加され、`tests/test_webtransport/root.rs` の `test_duplicate_operations_are_errors` が引き続き通ること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しないこと。
