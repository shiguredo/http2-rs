# driver の StopSending・終了理由・ピアリセットをアプリへ通知する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-notification
- Polished: {YYYY-MM-DD}

## 目的

0120 は `WtSessionEvent` で `SessionClosed` / `SessionDraining` を通知する設計だが、`WtEvent::StopSending` の送信側アプリへの伝達はスコープ外にしている。また driver はピアの正常終了も `Err(Error::ConnectionClosed)` として扱い、ピアの WT_RESET_STREAM は `Error::InvalidArgument` に変換して `error_code` / `reliable_size` を構造化しない。アプリが停止要求・正常終了・異常終了・ピアリセットを区別できるようにする。

## 0120 との依存関係

本 issue は 0120 が追加する `WtSessionEvent` と、`WtServerSession::next_session_event()` / `WtSessionParts::session_event_rx` を前提とする。0120 は open であり、現行の `crates/tokio-http2/src/webtransport.rs` に `WtSessionEvent` は存在しない。0120 完了後に着手する。0120 の `WtSessionEvent` の enum 名・variant 名・derive を変更しない。`StopSending` variant の追加は公開 enum の網羅 match を壊すため破壊的変更として扱う。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::dispatch_wt_event` は `WtEvent::StopSending { .. }` を「現在の API では送信側にシグナルを伝達しない (将来の拡張)」として無視する。

`DriverState::handle_event` は次の経路をすべて `Err(Error::ConnectionClosed)` で終了する。

- ピアの END_STREAM (`Event::DataReceived { end_stream: true }`)
- ピアの WT_CLOSE_SESSION (`WtSessionState::Closed` 検出後の END_STREAM 返信)
- CONNECT ストリームの `Event::StreamReset` / `Event::StreamClosed`

`DriverState::run` は `handle_event` の `Err` を `?` で伝播するため、`WtSessionParts::driver` を await しても、ピアの正常終了 (`END_STREAM` / `WT_CLOSE_SESSION`) と異常終了 (`CONNECT` の `StreamReset` / プロトコルエラー / I/O エラー) を区別できない。なお `cmd_rx` の閉塞と `conn.next_event()` の `Err(Error::ConnectionClosed)` は `Ok(())` で終了する。

`WtBidiStream::recv` / `WtUniRecvStream::recv` はピアの WT_RESET_STREAM を `Error::InvalidArgument(format!("stream reset by peer (error_code={error_code})"))` に変換し、`error_code` を文字列に埋め込む。`DriverState::dispatch_wt_event` の `WtEvent::StreamReset` は `reliable_size` を `..` で捨て、`StreamPacket::Reset` には `error_code` のみを載せる。

## 設計方針

### StopSending の通知

- 0120 の `WtSessionEvent` に `StopSending { stream_id: WtStreamId, error_code: u64 }` variant を追加する。`WtEvent::StopSending` のフィールドをそのまま載せる。
- `DriverState::dispatch_wt_event` の `WtEvent::StopSending` アームで `WtSessionEvent::StopSending` を 0120 の `mpsc::UnboundedSender<WtSessionEvent>` へ送る。
- アプリは `WtServerSession::next_session_event()` または `WtSessionParts::session_event_rx` で受信する。`WtBidiStream::send` / `WtUniSendStream::send` の戻り値は変更しない。sans-io 層は WT_STOP_SENDING 受信時に WT_RESET_STREAM を自動応答し、以後の送信は既存どおり `Error::WebTransport` になる。本 issue の通知は、アプリが送信ループを止めるための明示的なシグナルである。
- 通知するのはピア由来の `WtEvent::StopSending` のみ。ローカル `stop_sending()` では通知しない (0120 の「ローカル発信は通知しない」方針と整合)。

### 正常終了と異常終了の区別

- `WtSessionParts::driver` の型は `JoinHandle<Result<()>>` のまま変更しない。driver がピアの正常終了で終了した場合は `Ok(())`、異常終了で終了した場合は `Err` を返す。
- 正常終了として扱う経路:
  - ピアの END_STREAM (`Event::DataReceived { end_stream: true }`)
  - ピアの WT_CLOSE_SESSION (`WtSessionState::Closed` 検出後の END_STREAM 返信)
  - CONNECT ストリームの `Event::StreamClosed`
  - ローカルの `WtServerSession::close()` / `WtSessionHandle::close()` / `cmd_rx` の閉塞
  - `conn.next_event()` の `Err(Error::ConnectionClosed)`
- 異常終了として扱う経路:
  - CONNECT ストリームの `Event::StreamReset` (`Err(Error::ConnectionClosed)`)
  - `WtSession::feed` / `process` の `WtError` (既存の `Err(Error::WebTransport)`)
  - `conn.next_event()` の `Err` のうち `Error::ConnectionClosed` 以外 (I/O エラー等)
- `WT_CLOSE_SESSION` の `error_code` / `reason` は 0120 の `WtSessionEvent::Closed` で通知する。終了理由は 0120 の `WtSessionEvent` (enum) で返すため、driver の結果に別の終了理由 enum を追加しない (0120 の `Closed` と重複させない)。END_STREAM のみの終了は draft-ietf-webtrans-http2-16 Section 6.12 により `error_code` 0 / 空 reason の `WT_CLOSE_SESSION` と等価であり、`Ok(())` で正常終了を表す。
- `DriverState::handle_event` の戻り値は内部で「継続 / 正常終了 / 異常終了」を表せるように変更する。公開 API は変えない。
- `WtServerSession::close()` の `driver.await` 結果の扱い、`WtServerSession` の Drop の abort、`WtSessionParts` に Drop を追加しないこと (`driver` の `JoinHandle` の drop は detach) は変更しない。
- ここでいう正常終了・異常終了は、ピアまたはプロトコルに起因するセッション終了を指す。ローカルのコマンド処理失敗は呼び出し側へ ack で返る既存経路を維持し、driver の結果の区別には含めない。
- 0196 が追加する driver の終了結果を検証するテストは、正常終了 `Ok(())` / 異常終了 `Err` を前提に追従する。

### ピアリセットの構造化

- `crates/tokio-http2/src/error.rs` の `Error` に `StreamResetByPeer { error_code: u64, reliable_size: u64 }` variant を追加する。doc コメントに、sans-io 層の `WtEvent::StreamReset` が検証済みの Reliable Size を持ち、`reliable_size` がリセット前の受信済みバイト数と一致することを書く。
- `StreamPacket::Reset` に `reliable_size: u64` を追加する。`DriverState::dispatch_wt_event` の `WtEvent::StreamReset` で `error_code` と `reliable_size` を載せる。
- `WtBidiStream::recv` / `WtUniRecvStream::recv` は `StreamPacket::Reset` を受けたら `Err(Error::StreamResetByPeer { error_code, reliable_size })` を返し、`recv_finished` を true にする。
- `WtBidiStream::send` / `WtUniSendStream::send` / `reset` / `stop_sending` は変更しない。`Error` variant の追加は公開 enum の網羅 match を壊すため破壊的変更として扱う。

### 変更履歴とドキュメント

- `skills/shiguredo-http2/SKILL.md` の tokio-http2 WebTransport サーバー節に、`WtSessionEvent::StopSending`、driver の正常終了 `Ok(())` / 異常終了 `Err`、`Error::StreamResetByPeer` を追記する。0120 が追記する `WtSessionEvent` / `next_session_event()` / `session_event_rx` の記述を前提とする。
- `examples/wt_server/src/main.rs` の `run_echo` で `WtSessionEvent::StopSending` を受信したらログを出し、対象ストリームの送信タスクは既存の `WtBidiStream::send` / `WtUniSendStream::send` の `Error::WebTransport` で終了する。0120 が追加する `session_event_rx` の `tokio::select!` と `WtSessionEvent` の match を前提とし、既存の `Draining` / `Closed` の扱いを変えない。
- `CHANGES.md` は CODEBASE.md の凍結指示と 0199 の運用判断に従う。0199 が完了して凍結指示が解除されている場合は、`WtSessionEvent` の variant 追加と `Error::StreamResetByPeer` の追加を `[CHANGE]` として `## develop` に追記する。0199 が未完了なら変更しない。

## 完了条件

- ピアの WT_STOP_SENDING を受信したとき、`WtServerSession::next_session_event()` または `WtSessionParts::session_event_rx` で `WtSessionEvent::StopSending { stream_id, error_code }` を観測できること。`stream_id` と `error_code` はピアが送った値と一致すること。待機は `tokio::time::timeout` で上限を設け、固定 sleep を使わないこと。
- ピアの END_STREAM のみで終了したとき、`WtSessionParts::driver` を await した結果が `Ok(())` になること。
- ピアの WT_CLOSE_SESSION で終了したとき、`WtSessionEvent::Closed { error_code, reason }` を受信でき、`WtSessionParts::driver` を await した結果が `Ok(())` になること。`error_code` / `reason` はピアが送った値と一致すること。
- ピアが CONNECT ストリームを RST_STREAM で終了したとき、`WtSessionParts::driver` を await した結果が `Err` になること (`Ok(())` にならないこと)。
- ピアの WT_RESET_STREAM を受信したとき、`WtBidiStream::recv` または `WtUniRecvStream::recv` が `Err(Error::StreamResetByPeer { error_code, reliable_size })` を返し、`error_code` / `reliable_size` がピアが送った値と一致すること。`reliable_size` は受信済みバイト数と一致する値であること。
- 0120 の `SessionClosed` / `SessionDraining` 通知が維持されていること (`WtSessionEvent` の既存 variant と受け口の挙動を変えないこと)。
- `examples/wt_server/src/main.rs` の `run_echo` が `WtSessionEvent::StopSending` を受信してログを出し、`WtSessionEvent` の新しい variant を扱う分岐があり、`examples/wt_server` がコンパイルできること。
- `skills/shiguredo-http2/SKILL.md` に上記の API と挙動が追記されていること。
- `CHANGES.md` は 0199 の運用判断に従い、凍結指示が解除されている場合のみ `[CHANGE]` が追記されていること。
- テストは `crates/tokio-http2/tests/test_webtransport.rs` に追加し、上記の各経路を検証すること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。

## 参照

- `issues/0120-add-wt-event-user-notification.md` — `WtSessionEvent` と受け口の設計
- `crates/tokio-http2/src/webtransport.rs` — `DriverState::dispatch_wt_event` / `DriverState::handle_event` / `DriverState::run` / `WtBidiStream::recv` / `WtUniRecvStream::recv` / `StreamPacket::Reset`
- `crates/tokio-http2/src/error.rs` — `Error`
- `crates/tokio-http2/tests/test_webtransport.rs` — `test_wt_close_received_sends_end_stream` / `test_wt_stop_sending_inflight_data_does_not_abort_session` / `test_wt_max_data_non_increase_aborts_session`
- `examples/wt_server/src/main.rs` — `run_echo`
- `refs/draft-ietf-webtrans-http2-16.txt` — Section 3.4 (セッション終了) / Section 6.2 (WT_RESET_STREAM と Reliable Size) / Section 6.3 (WT_STOP_SENDING) / Section 6.12 (WT_CLOSE_SESSION と END_STREAM のみの終了の等価)
- `issues/closed/0170-change-wt-stream-reset-reliable-size.md` — `WtEvent::StreamReset` の `reliable_size` 追加。tokio 層への伝達は本 issue で行う
- `issues/0191-bug-fix-wt-session-parts-lifecycle.md` — `WtSessionParts::driver` を公開フィールドのまま残す
- `issues/0196-test-fix-wt-test-sleep-sync.md` — driver の終了結果を検証するテストの固定 sleep 置き換え
- `issues/0198-doc-update-wt-docs.md` / `issues/0199-doc-update-changes.md` — WT ドキュメント / CHANGES.md の運用
- `skills/shiguredo-http2/SKILL.md` — tokio-http2 WebTransport サーバー節

## pending にする理由

0120 が追加する `WtSessionEvent` と `WtServerSession::next_session_event()` / `WtSessionParts::session_event_rx` を前提とする。0120 は open で、現行の `crates/tokio-http2/src/webtransport.rs` に `WtSessionEvent` はまだ存在しない。0120 完了後に reopened にして、`WtSessionEvent` に `StopSending` variant を追加する。
