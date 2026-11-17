# WtSession の状態ガードを統一する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-session-state-guards
- Polished: 2026-11-17

## 目的

`WtSession` の送信系 API の状態検査が非対称である。`send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` はセッション状態を検査せず、`Closed` への遷移後も WT_CLOSE_SESSION の後ろに capsule を出力できる。また `process()` は `Initial` でも capsule を処理し、ストリームを生成する。draft-ietf-webtrans-http2-16 Section 3.2 はサーバーが WebTransport セッションを accept するまで request stream の capsule を処理してはならないとし (MUST NOT)、Section 6.12 は WT_CLOSE_SESSION を送信した端点が END_STREAM で half-close することを MUST とする。この 2 つの要件に合わせ、送信 API と受信処理の状態ガードを統一する。

## 現状

`src/webtransport.rs` の `send_stream_data` / `open_stream` / `send_datagram` / `send_max_streams` は `Active` / `Draining` を要求するが、`send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` はセッション状態を検査しない。`close()` は `state` を `Closed` にし、`streams` は削除しないため、`send_max_data` はストリームの有無にかかわらず、増加値を渡せば WT_MAX_DATA を `output_buffer` に追加でき、他の 3 API はストリームが残り、かつ各 API のストリーム状態・値の検証を通過すれば WT_MAX_STREAM_DATA / WT_RESET_STREAM / WT_STOP_SENDING を追加できる。特に `send_max_data` はストリームを必要としないため、`Closed` でも増加値を渡せば出力できる。`grow_recv_window` / `grow_stream_recv_window` は `send_max_data` / `send_max_stream_data` に委譲するため、同じ問題を継承する。`grow_max_streams` は既にガードを持つ `send_max_streams` に委譲するため、増分の検証を通過すれば `Active` / `Draining` 以外では既に `session_state_error` を返すが、増分の `checked_add` が状態検証より先行する。

`process()` は `Closed` のときだけ `handle_capsule` 冒頭で capsule を無視し、`Initial` では通常どおり `handle_capsule` を呼ぶ。そのため `Initial` で WT_STREAM を受信するとストリームを生成し、`StreamOpened` を送出する。`feed()` は状態を検査せず、`capsule_decoder` にバイト列を投入するだけである。`close()` は `Closed` 以外を拒否しないため `Initial` からも呼べ、`drain()` は `Active` のみ、`initiate()` は `Initial` のみを許可する。

`Closed` の吸収は closed 0132 で決定済みであり、`process()` は `Closed` で受信 capsule を無視して `Ok(())` を返す。`tests/test_webtransport/integration.rs` の `closed_session_ignores_subsequent_capsules` が、同一 `process()` 呼び出し内で WT_CLOSE_SESSION の後に続く capsule を無視することを検証している。`src/webtransport.rs` の `handle_capsule` 冒頭のコメントも、`Closed` でエラーを返すと driver が END_STREAM 応答に到達できないため吸収状態として無視すると説明している。

## 設計方針

- `send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` の先頭に共通の private 状態ガードを追加し、`Active` / `Draining` 以外では `session_state_error` を返す。ガードは値域・ストリーム存在・ストリーム状態の検証より先に評価し、拒否時は出力を生成せず、フロー制御値とストリーム状態も変更しない。既にガードを持つ `send_stream_data` / `open_stream` / `send_datagram` / `send_max_streams` の挙動は変えない。
- `grow_recv_window` / `grow_stream_recv_window` / `grow_max_streams` はメソッド先頭で共通ガードを呼ぶ。委譲先だけでは、`grow_recv_window` / `grow_max_streams` の `checked_add` と `grow_stream_recv_window` のストリーム存在確認が状態検証より先行するため、`Closed` / `Initial` でも `session_state_error` を返すには先頭ガードが必要である。これらの公開 API の rustdoc にも新しいエラー条件を追記する。
- `close()` は `Active` / `Draining` のみ許可し、`Initial` と `Closed` では `session_state_error` を返す。`initiate()` はローカル状態を `Initial` から `Active` へ遷移させるだけで、クライアントは 2xx 応答を待たずに呼べる。したがって draft Section 3.2 の楽観的 capsule 送信 (MAY) は `Active` で引き続き可能であり、`Initial` は「まだ `initiate()` していない」状態として扱う。`drain()` は従来どおり `Active` のみとする。
- `process()` は `Initial` では何もデコードせず `session_state_error` を返す。`feed()` 済みのバイト列は `capsule_decoder` に保持され、`initiate()` 後の `process()` で処理される。`Closed` では従来どおり capsule をデコードして無視し `Ok(())` を返す（closed 0132 の吸収契約を維持する）。`Active` / `Draining` では従来どおり処理する。`handle_capsule` 冒頭の `Closed` ガードは、同一 `process()` 呼び出し内で WT_CLOSE_SESSION の後に続く capsule を吸収するために残す。`crates/tokio-http2/src/webtransport.rs` の server は 2xx 応答送信後に `initiate()` するため、driver の受信経路は `Active` で `process()` を呼び、本変更の影響を受けない。
- `feed()` は状態を理由にしたエラーを返さない。Section 3.2 が禁じるのは capsule の「処理」であり、`feed()` はデコーダへの入力だけを行う。これにより、サーバーが accept 前に届いた DATA を保持し、accept 後に `process()` で処理できる。
- 変更する公開 API の rustdoc を更新する。`send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` / `close` / `grow_recv_window` / `grow_stream_recv_window` / `grow_max_streams` / `process` / `feed` に、`session_state_error` の条件、検証順、拒否時に状態を変更しないことを明記する。特に `send_max_stream_data` の rustdoc は現在「varint 上限超過、ストリームの存在、状態、非増加」の順を明記しているため、セッション状態の検証が先頭に来ることを反映する。`grow_stream_recv_window` と `grow_max_streams` の rustdoc の検証順も同様に更新する。
- テストは `tests/test_webtransport/integration.rs` に追加する。`root.rs` に置く方が自然な `close()` / `initiate()` の状態遷移テストは `tests/test_webtransport/root.rs` に追加する。テストでは、`Initial` の `process()` が `session_state_error` を返してストリームとイベントを生成せず、`feed()` 済みデータが `initiate()` 後の `process()` で処理されること、`Initial` の `close()` が `session_state_error` を返して出力を生成しないこと、`Closed` の 4 API が `session_state_error` を返して出力と状態を変更しないこと、`Closed` の `process()` が capsule を無視して `Ok(())` を返すこと、`Draining` の 4 API と `close()` が受理されること、`Draining` の `process()` が capsule を処理すること、`Active` の 4 API が従来どおり受理されること、`grow_recv_window` / `grow_stream_recv_window` / `grow_max_streams` の状態エラーを検証する。
- `CODEBASE.md` の指示に従い、`CHANGES.md` は変更しない。

## 完了条件

- `Closed` 後に `send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` が `session_state_error` を返し、出力を生成せず、フロー制御値とストリーム状態を変更しないこと。
- `Initial` では `send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` が `session_state_error` を返し、出力を生成せず、状態を変更しないこと。ストリーム不在の `send_max_stream_data` / `reset_stream` / `stop_sending` でも、`invalid_stream_id` や値域の検証より先に `session_state_error` が返ること。
- `Initial` からの `close()` が `session_state_error` を返し、出力を生成しないこと。`Draining` からの `close()` は受理されること。
- `Initial` での `process()` が `session_state_error` を返し、ストリームとイベントを生成しないこと。`feed()` 済みの capsule が保持され、`initiate()` 後の `process()` で処理されること。`Draining` での `process()` は従来どおり capsule を処理すること。この契約が `process()` の rustdoc に明記されていること。
- `Closed` での `process()` が受信 capsule を無視して `Ok(())` を返すこと（closed 0132 の非回帰）。`feed()` は `Initial` / `Closed` でも状態を理由にしたエラーを返さず、データを保持すること。
- `Draining` では `send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` が受理されること。`Active` でも従来どおり受理されること。
- `grow_recv_window` / `grow_stream_recv_window` / `grow_max_streams` が `Closed` / `Initial` で `session_state_error` を返し、出力を生成せず、状態を変更しないこと。`grow_stream_recv_window` はストリーム存在確認より先にセッション状態を検証すること。
- 変更する公開 API の rustdoc が実装と一致していること。特に `send_max_stream_data` / `grow_stream_recv_window` / `grow_max_streams` の検証順、`process()` の `Initial` / `Closed` の契約、`feed()` の状態非依存が明記されていること。
- 上記のテストが追加されていること。
- `CHANGES.md` を変更していないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。

## 参照

- `src/webtransport.rs` — `WtSession::process` / `WtSession::feed` / `WtSession::close` / `WtSession::send_max_data` / `WtSession::send_max_stream_data` / `WtSession::reset_stream` / `WtSession::stop_sending` / `WtSession::grow_recv_window` / `WtSession::grow_stream_recv_window` / `WtSession::grow_max_streams` / `WtSession::handle_capsule`
- `crates/tokio-http2/src/webtransport.rs` — `WtServerRequest::accept` の `initiate()` 呼び出し位置
- `tests/test_webtransport/integration.rs` — `closed_session_ignores_subsequent_capsules` / `send_max_streams_closed_session_errors` / `send_max_streams_session_state_errors`
- `tests/test_webtransport/root.rs` — `test_close_session` / `test_close_double_call_errors` / `test_drain_session` / `test_draining_state_allows_operations`
- `refs/draft-ietf-webtrans-http2-16.txt` — Section 3.2 (Creating a New Session) / Section 6.12 (WT_CLOSE_SESSION Capsule)
- `issues/closed/0132-bug-wt-closed-recv-capsule.md` — `Closed` 状態の吸収契約
