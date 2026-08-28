# WtEvent の SessionClosed / SessionDraining をユーザーに通知する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/add-wt-event-user-notification
- Polished: 2026-08-28

## 目的

ピアから送信された `WT_DRAIN_SESSION` / `WT_CLOSE_SESSION` を、`tokio-http2` のサーバー側ユーザーコードに通知する経路を追加する。

必要なのは通知の「有無」ではなく、次の 2 点を区別・取得できることである。

- `WT_DRAIN_SESSION`: 完全に不観測。draft-ietf-webtrans-http2-15 Section 6.13 は "After sending or receiving either a WT_DRAIN_SESSION capsule or HTTP/2 GOAWAY frame, an endpoint MAY continue using the session and MAY open new WebTransport streams. The signal is intended for the application using WebTransport, which is expected to attempt to gracefully terminate the session as soon as possible." と定め、信号の受け手はアプリケーションである。通知経路が無いと仕様上の意図を満たせない
- `WT_CLOSE_SESSION`: driver の終了としては観測できるが、ピア由来の終了であることと `error_code` / `reason` が失われる。draft-15 Section 3.4 は "either endpoint can send a WT_CLOSE_SESSION capsule with an application error code and message to convey additional information about the reasons for the closure of the session" と定め、伝達すべき情報はアプリに届いていない

## 現状

- Sans I/O 層 (`src/webtransport.rs` の `WtEvent` enum) には `SessionDraining` と `SessionClosed { error_code, reason }` が定義され、`WtSession::handle_capsule` がピア由来のカプセル受信時に発行する
  - `Capsule::WtCloseSession` は `state != Closed` のとき `WtEvent::SessionClosed` を発行 (`Closed` は吸収状態)
  - `Capsule::WtDrainSession` は `state == Active` のとき `WtEvent::SessionDraining` を発行 (`Draining -> Draining` は冪等)
  - ローカル発信の `WtSession::close()` / `WtSession::drain()` はカプセルをエンコードして状態を変えるだけで、`WtEvent` を発行しない
- `tokio-http2` 層 (`crates/tokio-http2/src/webtransport.rs` の `DriverState::dispatch_wt_event`) では、`WtEvent::SessionDraining` と `WtEvent::SessionClosed { .. }` が単一の match アームにまとめられ、`// 何もしない (ユーザーに close/drain を通知する手段は将来追加)` のコメントのみがある。`WtEvent::StopSending` は別のアームで `// 現在の API では送信側にシグナルを伝達しない (将来の拡張)` のコメントのみ
- ピア由来の `WT_CLOSE_SESSION` に対しては `DriverState::handle_event` が `WtSessionState::Closed` を検出して END_STREAM を返信し (draft-15 Section 6.12 の MUST)、`Err(Error::ConnectionClosed)` を返して `DriverState::run` が終了する。これで `bidi_tx` / `uni_tx` / `datagram_tx` がドロップされるため、ユーザー側は `accept_bidi()` / `accept_uni()` / `recv_datagram()` が `None` を返すこと、または `WtSessionParts::driver` を await することで終了自体は検知できる。失われるのは終了理由の区別と `error_code` / `reason` の値である
- `WtServerSession` は `bidi_rx` / `uni_rx` / `datagram_rx` / `driver` を private フィールドに持つ一方、`WtServerSession::into_parts()` は `WtSessionParts` (`session_id` / `selected_protocol` / `bidi_rx` / `uni_rx` / `datagram_rx` / `handle` / `driver` のすべてが `pub`) を返す。リポジトリ内唯一の実ユーザーコードである `examples/wt_server/src/main.rs` の `run_echo` は `into_parts()` の結果を `..` 付きの分割代入で束縛し (`bidi_rx` / `uni_rx` / `datagram_rx` / `handle` / `driver` を束縛して残りを破棄している)、`recv()` が `None` を返すことでセッション終端を判定し、終端後に `let _ = driver.await;` で driver の結果を捨てている
- `DriverState::handle_event` は `while let Some(wt_ev) = self.wt_session.poll_event()` のループで `dispatch_wt_event()` を呼び、その**後に** `WtSessionState::Closed` の判定で END_STREAM 返信と `Err(Error::ConnectionClosed)` を行う。つまり通知を `dispatch_wt_event()` で送れば、driver 終了より前にキューへ積める
- `crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_close` / `test_wt_drain` はサーバーからクライアントへの送信方向のみを検証し、ピア由来カプセルの受信方向は `test_wt_close_received_sends_end_stream` (sans-io の `WtSession::client()` をピアに見立てて `WT_CLOSE_SESSION` を流す) が END_STREAM 返信のみを検証している。イベントがユーザーに届くことは検証されていない
- ドレイン時の挙動は仕様どおり実装済み。`src/webtransport.rs` の `WtSession::open_bidi_stream()` / `open_uni_stream()` / `send_stream_data()` / `send_datagram()` は `WtSessionState::Draining` を許可し、Section 6.13 を根拠コメントに持つ。ピア開始ストリームの受信もセッション状態を参照しない

## 設計方針

### 通知の形 (push 型チャネルに確定)

- `DriverState` が `mpsc::UnboundedSender<WtSessionEvent>` を 1 個所有し、`dispatch_wt_event` の該当アームから送る。unbounded なので driver 終了後も配送済みの通知は受信側で消費できる
- 通知専用 enum を `crates/tokio-http2/src/webtransport.rs` に新規定義する

  ```
  pub enum WtSessionEvent {
      Draining,
      Closed { error_code: u32, reason: String },
  }
  ```

- `shiguredo_http2::webtransport::WtEvent` を再 export しない。`WtEvent` のストリーム系・DATAGRAM 系 variant は `bidi_rx` / `uni_rx` / `datagram_rx` / `stream_channels` ですでに別の形へ変換されており、同一 enum を流すと 1 variant だけチャネルが重複する不整合になる。shiguredo-rust の「re-export は基本的にやらないこと」にも従う
- driver に問い合わせる pull 型メソッド (`closed_reason() -> Option<...>` 等) にしない。ピア close 受信後に driver は `Err(Error::ConnectionClosed)` で終了するため、終了後は応答できず、`cmd_tx` 往復でも同じ理由で失われる。`Arc<Mutex<..>>` での共有状態は shiguredo-rust が最終手段としているため採用しない
- 受け口は未分解・分解済みの両方に提供する
  - `WtServerSession` に `pub async fn next_session_event(&mut self) -> Option<WtSessionEvent>` を追加する (内部の `rx.recv()`。チャネルが閉じたら `None`)
  - `WtSessionParts` に `pub session_event_rx: mpsc::UnboundedReceiver<WtSessionEvent>` を追加し、`into_parts()` で移譲する
  - `examples/wt_server/src/main.rs` の `run_echo` は `WtSessionParts` を `..` 付きで分割代入しているため、フィールドを追加してもコンパイルは通り、`session_event_rx` は無警告で破棄される (clippy でも検出されない)。したがって `run_echo` 側で明示的に束縛し直す作業が必須である
- 通知の送信は `DriverState::dispatch_wt_event` の `WtEvent::SessionDraining | WtEvent::SessionClosed { .. }` アームでのみ行う。`DriverState::handle_event` 側では送らない (`handle_event` は `dispatch_wt_event()` を呼んだ後に END_STREAM 返信と `Err(Error::ConnectionClosed)` を行うため、通知は driver 終了より先にキューへ積まれる)
- 新規公開 enum とメソッドには `///` doc コメントを付ける (shiguredo-rust の公開 API doc 必須)。`next_session_event()` と `session_event_rx` の doc には「ピア由来の `WT_CLOSE_SESSION` / `WT_DRAIN_SESSION` のみを通知し、ローカル発信の `close()` / `drain()` では発生しない」こと、および「ローカル発信で `Draining` になったセッションへピアから `WT_DRAIN_SESSION` が届いても、Sans I/O 層が `state == Active` のときしか `SessionDraining` を発行しないため通知も来ない」ことを書く

### 通知する範囲

- 通知するのはピア由来の `WtEvent::SessionDraining` / `WtEvent::SessionClosed` の受信のみとする
- ローカル発信の `WtServerSession::drain()` / `WtServerSession::close()` は通知しない。Sans I/O 層が発行しないイベントであり、通知すると意図的な操作がピアからの signal として返る
- END_STREAM のみの正常終了は `WtSessionEvent::Closed { error_code: 0, reason: String::new() }` として合成しない。draft-15 Section 6.12 は END_STREAM のみの終了を `error_code` 0 / empty reason の `WT_CLOSE_SESSION` と意味論的に等価と定めるが、現状 `DriverState::handle_event` は `Err(Error::ConnectionClosed)` を返すだけで `WtEvent::SessionClosed` を発行しない。チャネル閉塞と driver の `JoinHandle` の戻り値で検知可能なため、合成による通知は行わない
- HTTP/2 GOAWAY 由来の drain 信号 (Section 6.13 が `WT_DRAIN_SESSION` と並列で挙げる) は対象外。`DriverState` は GOAWAY を `WtEvent` に変換していないため、対応するには別途受信経路の追加が必要であり本 issue の目的外である

### 挙動を変えないこと

- `WtSessionEvent::Draining` の通知をトリガーに、driver が新規ストリームの受付を遮断したり自動で close したりしない。Section 6.13 は新規ストリーム開設を許可しており、現状の `open_bidi_stream()` / `open_uni_stream()` / `send_stream_data()` / `send_datagram()` の Draining 許容と一致している。graceful 終了の判断は通知を受け取ったアプリケーションが `close()` を呼んで行う
- `WtSessionEvent::Closed` の通知でも `DriverState::handle_event` の END_STREAM 返信と `Err(Error::ConnectionClosed)` の順序を変えない。draft-15 Section 6.12 の MUST である END_STREAM 返信は `end_stream` 判定より前に置く既存の順序を維持し、通知は `dispatch_wt_event` 側で `tx.send()` するだけにする (上記のとおり `handle_event` は `dispatch_wt_event()` を終えてから END_STREAM 返信へ進むため、既存の順序と両立する)

## スコープ外

- `WtEvent::StopSending` の送信側アプリへの伝達。Sans I/O 層の WT_RESET_STREAM 自動応答は `issues/closed/0059-bug-fix-wt-stop-sending-auto-reset.md` で決着済みで、アプリ側は `WtBidiStream::send()` が `Error::WebTransport` (`stream_state_error` / `invalid_stream_id`) を返すことで観測できる。専用シグナルの追加は別の設計判断であり本 issue では扱わない
- `WtServerSession::close()` が driver への送信結果を無視している問題は `issues/closed/0121-bug-wt-close-error-propagation.md` で対応済み (`cmd_tx.send()` と `rx.await` の結果を伝播する現在形)。残る同経路の問題 (送信ウィンドウ枯渇時に `Ok` を返す) は `issues/0138-bug-wt-close-silent-not-send.md` が扱うため、本 issue では扱わない

## 他 issue との関係

本 issue と同じ `crates/tokio-http2/src/webtransport.rs` (特に `DriverState`) を触る open issue が複数ある。着手順に関係なく、リベースで同一アームの競合が出る前提で進めること。

- `issues/0129-bug-wt-stop-sending-inflight-abort.md` — `DriverState::dispatch_wt_event` の `WtEvent::StreamData` 経路と `grow_stream_recv_window` のエラー扱いを変更する。同一メソッドの別アームだが、修正が `dispatch_wt_event` 全体に及ぶため後着手側が追従する
- `issues/0130-bug-wt-grow-stream-window-initial-value.md` — `DriverState::maybe_grow_stream_window` の初期値を扱う。`dispatch_wt_event` 内の呼び出しと近接する
- `issues/0138-bug-wt-close-silent-not-send.md` — `DriverState::handle_cmd` の Close 経路と `flush_wt_output()` を変更する。`DriverState` の構造体フィールド追加 (受信箱 sender) と同一 impl ブロックで衝突しうる
- `issues/0132-bug-wt-closed-recv-capsule.md` — Sans I/O 側 (`src/webtransport.rs`) の Closed 状態での受信 capsule 処理を変える。本 issue の「現状」が書く `WtSession::handle_capsule` の状態ゲート条件が古くなる可能性があるため、0132 完了後は本 issue の現状記述を再確認する
- `issues/0117-change-privatize-wt-config-fields.md` — `DriverState::maybe_grow_session_window` / `maybe_grow_max_streams` の `WtConfig` フィールドアクセスを getter 経由へ書き換える。同じファイルのためリベースを見込む
- 0121 は closed 済み。`WtServerSession::close()` の ack 経路は現在形になっており、本 issue はそれを前提に `WtServerSession` へのメソッド追加を行う

## 完了条件

- `WtSessionEvent` (`Draining` / `Closed { error_code, reason }`) が `crates/tokio-http2` の公開型として定義され、`crates/tokio-http2/src/lib.rs` の `pub use webtransport::{...}` から到達できる
- `WtServerSession::next_session_event()` と `WtSessionParts::session_event_rx` の両方から通知を消費できる
- `DriverState::dispatch_wt_event` の `WtEvent::SessionDraining | WtEvent::SessionClosed { .. }` アームの「将来追加」コメントが消え、通知の送信が実装されている
- ローカル発信では通知が発生しないことを検証するテストが `crates/tokio-http2/tests/test_webtransport.rs` にある。`WtServerSession::close()` は `self` を消費して driver を await するため、検証は `into_parts()` 経由で行う: `WtSessionHandle::drain()` の後は `WtSessionParts::session_event_rx.try_recv()` が `Empty` を返すこと、`WtSessionHandle::close()` の後は driver が終了して `session_event_rx.recv()` が `None` を返すこと (`Some(WtSessionEvent::Closed { .. })` にならないこと) で決定的に検証する
- ピア由来の `WT_CLOSE_SESSION` について、`crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_close_received_sends_end_stream` と同じ組み立て (sans-io の `WtSession::client()` をピアに見立てる) で `WtSessionEvent::Closed` が受信でき、`error_code` と `reason` の値が送信側と一致することを検証するテストが追加されている。待機は必ず `tokio::time::timeout` で上限を設けて `recv()` する (既存テストにある固定長の `tokio::time::sleep` は踏襲しない。通知漏れ時にテストがハングするのを防ぎ、合否を決定的にするため)
- ピア由来の `WT_DRAIN_SESSION` について、`WtSessionEvent::Draining` が受信できることを検証するテストが追加されている (同上の待ち方)。加えて通知後も `WtServerSession::open_bidi()` (または `WtSessionParts::handle` 経由の `WtSessionHandle::open_bidi()`) による双方向ストリームの開設と送信が継続できること (Section 6.13 の挙動) を同じテストで検証する
- `examples/wt_server/src/main.rs` が `WtSessionEvent` を実際に消費している (drain 受信と close 受信に対する挙動を持つ。`CODEBASE.md`「公開 API は必ず使用箇所とテストを用意すること」)
- ドレイン通知による新規ストリーム遮断・自動 close が導入されていない (`src/webtransport.rs` の Draining 許可挙動と `DriverState::handle_event` の END_STREAM 順序が不変)
- 新規公開 API すべてに `///` doc コメントが付いている
- `skills/shiguredo-http2/SKILL.md` の「tokio-http2 WebTransport サーバー」節と `WtEvent` 関連の節に、`WtSessionEvent` と受け口が追記されている
- `CHANGES.md` の `## develop` に 2 エントリが追加されている (`[CHANGE]` から `[ADD]` の順): `WtSessionParts::session_event_rx` の `pub` フィールド追加は網羅的な分割代入を利用側で壊すため `[CHANGE]`、`WtSessionEvent` と `WtServerSession::next_session_event()` の追加は `[ADD]`。いずれも次の行に担当者行を置く (`issues/0117-change-privatize-wt-config-fields.md` と同時期に `## develop` へ追記する issue があるため、編集コンフリクトに注意)
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `crates/tokio-http2/src/webtransport.rs` — `DriverState::dispatch_wt_event` / `DriverState::handle_event` / `WtServerSession` / `WtSessionParts` / `DriverCmd`
- `src/webtransport.rs` — `WtEvent` / `WtSession::handle_capsule` / `WtSession::close` / `WtSession::drain` / `WtSessionState`
- `crates/tokio-http2/tests/test_webtransport.rs` — `test_wt_close` / `test_wt_drain` / `test_wt_close_received_sends_end_stream`
- `examples/wt_server/src/main.rs` — `into_parts()` によるセッション分解と終端判定
- `refs/draft-ietf-webtrans-http2-15.txt` — Section 3.4 (WT_CLOSE SESSION の目的) / Section 6.12 (END_STREAM 返信の MUST と END_STREAM のみ終了の等価) / Section 6.13 (WT_DRAIN_SESSION Capsule)
- `issues/closed/0121-bug-wt-close-error-propagation.md` — close のエラー伝播 (closed 済み)
- `issues/closed/0058-bug-fix-wt-close-session-end-stream.md` / `issues/closed/0061-bug-fix-wt-close-session-reason-truncation.md` / `issues/closed/0085-change-wt-draft15-session-semantics.md` — WT_CLOSE_SESSION の受信時 END_STREAM 返信・reason 切り詰め・draft-15 セッション状態意味論の先行対応
- `issues/closed/0059-bug-fix-wt-stop-sending-auto-reset.md` — StopSending の自動応答が決着した issue
- CODEBASE.md / shiguredo-rust スキル — 公開 API の使用箇所とテスト、re-export をしない、共有状態よりチャネル所有
