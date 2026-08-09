# 破棄された DATA の接続ウィンドウ消費をアプリが補充できず枯渇する問題を修正する

- Priority: Medium
- Created: 2026-08-08
- Completed: 2026-08-09
- Branch: feature/change-connection-window-exhaustion
- Polished: 2026-08-08

## 目的

`Connection::handle_data` は、ストリームエラーで破棄する DATA もクローズ済みストリームへの遅延 DATA 破棄も接続フロー制御ウィンドウに計上する (RFC 9113 Section 6.9 の MUST)。しかし破棄経路では `Event::DataReceived` が生成されず、`Event::StreamReset` には消費バイト数が含まれないため、アプリは何バイト補充すべきかを原理的に知ることができない。悪意のあるピアが Content-Length 超過の DATA を送り続けると (違反後に同一ストリーム ID へ DATA を送り続けることでクローズ済み破棄経路にも入る)、接続ウィンドウ (デフォルト 65535) が枯渇し、後続の DATA 受信が FLOW_CONTROL_ERROR の接続エラーで遮断される。

本修正でストリームエラーが RST_STREAM 処理に変換され接続が維持されるようになったことで、この経路が実用的な攻撃面として顕在化した。繰り返し違反に対する接続維持が成立するよう、破棄データのウィンドウ補充手段を提供する。

## 現状

- `Connection::handle_data` はフロー制御ウィンドウの計上を `self.flow_control.consume_recv(flow_control_size)` (`src/connection.rs`) で全経路に先立って行う (RFC 9113 Section 6.9 の MUST)。エラー経路 (状態遷移違反 / ストリームレベル フロー制御違反 / no-content 違反 / Content-Length 超過 / END_STREAM 時不一致の 5 経路) とクローズ済みストリームへの遅延 DATA 破棄経路では、計上されたウィンドウが永久に回復しない
- `FlowControl::should_send_window_update` / `FlowControl::window_update_increment` (`src/flow_control.rs`) は存在するが、`src/connection.rs` から一切呼ばれていない
- `Event::StreamReset` (`src/event.rs`) は `stream_id` と `error_code` のみで、消費バイト数を持たない
- `crates/tokio-http2` 層では WebTransport セッション (`crates/tokio-http2/src/webtransport.rs` の `handle_event`) のみ `Event::DataReceived` の `data.len()` に応じて WINDOW_UPDATE を送信する。それ以外の通常 HTTP/2 サーバー・クライアントはアプリが `send_window_update` を手動呼び出しする設計
- 既存のクローズ済みストリームへの遅延 DATA 破棄経路にも同種の穴はあったが、ストリームエラーが接続終了を引き起こしていたため実用的な攻撃経路ではなかった

## 設計方針

### 採用: 案 A (イベントに接続ウィンドウ消費バイト数を追加)

破棄・リセット経路で消費された接続ウィンドウのバイト数をイベントで通知し、アプリが補充量を認識できるようにする。Sans I/O 層の既存設計 (イベント生成のみ・WINDOW_UPDATE 送信はアプリ判断) と整合する。

設計の詳細:

- `Event::StreamReset` に `connection_window_consumed: usize` フィールドを追加する (接続フロー制御ウィンドウに計上されたバイト数)。ストリームエラー経路 (状態遷移違反 / ストリームレベル フロー制御違反 / no-content 違反 / Content-Length 超過 / END_STREAM 時不一致の 5 経路) で、破棄する DATA の `flow_control_size` を `reset_stream` に渡して反映する
- クローズ済みストリームへの遅延 DATA 破棄経路は `Event::StreamReset` を生成しない。`Event::StreamReset` の意味論は「RST_STREAM 送受信と対称」であり、RST_STREAM を送信しない破棄のみの経路で発行すると意味論が壊れる (0101 の `test_stream_error_reset_delayed_data_discarded` が「リセット済みストリームへの遅延 DATA で Event::StreamReset が再発してはならない」を検証済み)。代わりに専用イベント `Event::DataDiscarded { stream_id, connection_window_consumed: usize }` を新設して通知する。破棄経路の判定は「`streams` マップに存在しない場合」に加えて「マップ内で Closed 状態の場合」を含む。後者は `recv_headers` のエラー経路 (1xx + END_STREAM 等の malformed 検出で状態遷移 (HalfClosedLocal + END_STREAM → Closed) 後に Err を返し、`streams` からの削除処理に到達しないケース) で発生し、このストリームへの遅延 DATA も同様に破棄して RST_STREAM 送信・`Event::StreamReset` 再発を防ぐ
- 受信パス (`handle_rst_stream`) は RST_STREAM 受信自体が DATA ではないため `connection_window_consumed: 0`
- 公開 API `reset_stream` は呼び出し側が消費バイト数を知らないため `connection_window_consumed: 0` とする (内部の `handle_data` からの違反処理は専用の内部経路でバイト数を渡す)
- アプリは `Event::StreamReset` / `Event::DataDiscarded` の `connection_window_consumed` ぶん `send_window_update(StreamId::Connection, ...)` を送って補充する (0 の場合は送信不要)
- 本 issue は Sans I/O 層のイベント追加のみを行う。`crates/tokio-http2` / `examples/` 等の利用者側で新フィールドを処理する更新は別途の対応とする (本 issue のスコープ外)

## 完了条件

- `Event::StreamReset` に `connection_window_consumed: usize` フィールドが追加されている
- `Event::DataDiscarded { stream_id, connection_window_consumed: usize }` が新設されている
- `Connection::handle_data` のストリームエラー 5 経路 (状態遷移違反 / ストリームレベル フロー制御違反 / no-content 違反 / Content-Length 超過 / END_STREAM 時不一致) で破棄する DATA の `flow_control_size` が `Event::StreamReset` の `connection_window_consumed` に反映されている
- クローズ済みストリームへの遅延 DATA 破棄経路 (「`streams` マップに存在しない場合」と「マップ内で Closed 状態の場合」の両形態) で `Event::DataDiscarded { stream_id, connection_window_consumed: flow_control_size }` が生成されている (空 DATA の場合は `connection_window_consumed: 0` のイベントを生成しない)
- 0101 の `test_stream_error_reset_delayed_data_discarded` (遅延 DATA で `Event::StreamReset` が再発しないこと) が維持されている
- 受信パス (`handle_rst_stream`) と公開 API `reset_stream` は `connection_window_consumed: 0` で `Event::StreamReset` を生成する
- 既存の `Event::StreamReset` パターンマッチ箇所 (`tests/test_connection.rs` / `pbt/tests/prop_event.rs` / `crates/tokio-http2/tests/`) と `skills/shiguredo-http2/SKILL.md` のイベント一覧が新フィールド・新バリアントに追従している。`src/event.rs` の `stream_id()` に `DataDiscarded` を追加し (ストリームレベルイベントなので `Some(*stream_id)` を返す)、`pbt/tests/prop_event.rs` の `stream_level_event()` Strategy に `DataDiscarded` を組み込む
- 違反 DATA を繰り返し送信しても接続ウィンドウが枯渇せず、正当なストリームの DATA 受信が継続できること (アプリが `connection_window_consumed` ぶん補充した場合)
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ること。ストリームエラー 5 経路それぞれの `connection_window_consumed` が正しく通知されることを検証するテストを含む

## 解決方法

1. `src/event.rs` の `Event::StreamReset` に `connection_window_consumed: usize` フィールドを追加し、`Event::DataDiscarded { stream_id, connection_window_consumed: usize }` を新設する
2. `src/connection.rs` の `handle_data` のストリームエラー 5 経路 (状態遷移違反 / ストリームレベル フロー制御違反 / no-content 違反 / Content-Length 超過 / END_STREAM 時不一致) で、破棄する DATA の `flow_control_size` を `Event::StreamReset` の `connection_window_consumed` に反映する。内部経路は `reset_stream_internal` を新設してバイト数を渡し、公開 API `reset_stream` はバイト数を知らないため `0` を渡す形で共通化し、RST_STREAM 送信・`closed_streams` 登録・`streams` 削除・イベント生成の一連の処理を重複させない
3. クローズ済みストリームへの遅延 DATA 破棄経路で `Event::DataDiscarded { stream_id, connection_window_consumed: flow_control_size }` を生成する (空 DATA の場合は生成しない)
4. `handle_rst_stream` と公開 API `reset_stream` の `Event::StreamReset` 生成に `connection_window_consumed: 0` を設定する
5. 既存の `Event::StreamReset` パターンマッチ箇所 (`tests/test_connection.rs` / `pbt/tests/prop_event.rs` / `crates/tokio-http2/tests/interop.rs` / `client_server.rs` / `test_webtransport.rs`) と `skills/shiguredo-http2/SKILL.md` のイベント一覧を新フィールド・新バリアントに追従させる。`src/event.rs` の `stream_id()` に `DataDiscarded` を追加し、`pbt/tests/prop_event.rs` の `stream_level_event()` Strategy に `DataDiscarded` を組み込む
6. 単体テストを追加する:
   - ストリームエラー 5 経路それぞれで `Event::StreamReset` の `connection_window_consumed` が `flow_control_size` と一致することを検証する
   - 攻撃シナリオの経路遷移 (1 回目の違反 DATA (Content-Length 超過) で `Event::StreamReset` の `connection_window_consumed` が通知され、その後同一ストリーム ID への遅延 DATA で `Event::DataDiscarded` が通知されること) を検証する。`DataDiscarded` は「`streams` マップに存在しない場合」と「マップ内で Closed 状態の場合」の両形態をカバーする (後者は `recv_headers` のエラー経路 (1xx + END_STREAM malformed) で発生する)
   - 補充しない場合に接続ウィンドウが枯渇して `FLOW_CONTROL_ERROR` の接続エラーで遮断されること (動機の再現) と、補充した場合に正当な DATA が受信できること (修正の効果) の対比を検証する
7. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の先頭に以下のエントリを追加する (リポジトリの慣習どおり新しいエントリを上に置く):

   ```markdown
   - [CHANGE] `Event::StreamReset` に接続ウィンドウ消費バイト数を追加し、`Event::DataDiscarded` を新設する。ストリームエラー / 遅延 DATA 破棄で消費された接続ウィンドウのバイト数をアプリが認識して補充できるようにする (RFC 9113 Section 6.9)
     - @voluntas
   ```

8. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ることを確認する

## 解決方法

1. `src/event.rs` の `Event::StreamReset` に `connection_window_consumed: usize` フィールドを追加し、`Event::DataDiscarded { stream_id, connection_window_consumed: usize }` を新設した。`stream_id()` に `DataDiscarded` を追加し (ストリームレベルイベントとして `Some(*stream_id)` を返す)、フィールド doc に 2^31-1 上限超過時の FLOW_CONTROL_ERROR についての注記を追記した
2. `src/connection.rs` に `reset_stream_internal` を新設し、公開 API `reset_stream` は `0` を渡す形で共通化した。`handle_data` のストリームエラー 5 経路 (状態遷移違反 / ストリームレベル フロー制御違反 / no-content 違反 / Content-Length 超過 / END_STREAM 時不一致) で、破棄する DATA の `flow_control_size` (Pad Length フィールド + データ + パディング) を `Event::StreamReset` の `connection_window_consumed` に反映した
3. `handle_data` 冒頭のクローズ済みチェックで、破棄する遅延 DATA の接続ウィンドウ消費量を `Event::DataDiscarded` で通知するようにした (空 DATA は消費 0 のため通知しない)。判定は「`streams` マップに存在しない場合」に加えて「マップ内で Closed 状態の場合」を含む。後者は `process_headers` のエラー経路 (1xx + END_STREAM 等の malformed 検出で状態遷移後に Err を返し、`streams` からの削除処理に到達しないケース) で発生し、このストリームへの遅延 DATA も同様に破棄して RST_STREAM 送信・`Event::StreamReset` 再発を防ぐ
4. `handle_rst_stream` と公開 API `reset_stream` は `connection_window_consumed: 0` で `Event::StreamReset` を生成する
5. 既存の `Event::StreamReset` パターンマッチ箇所 (`tests/test_connection.rs` / `pbt/tests/prop_event.rs` / `crates/tokio-http2/tests/interop.rs` / `client_server.rs` / `test_webtransport.rs`) と `skills/shiguredo-http2/SKILL.md` のイベント一覧を新フィールド・新バリアントに追従させた。`pbt/tests/prop_event.rs` の `stream_level_event()` Strategy に `DataDiscarded` を組み込み、Strategy の範囲を実装に合わせた (DataDiscarded は 0 を生成しない)
6. `tests/test_connection.rs` に単体テストを追加した:
   - `assert_internal_reset` に `expected_consumed` を追加し、ストリームエラー 5 経路それぞれの `connection_window_consumed` を検証 (Content-Length 超過 6 / END_STREAM 時不一致 3 / no-content 違反 3 / HalfClosedRemote 状態への DATA 3 / フロー制御違反 4097)。`DataDiscarded` 非生成 (StreamReset との排他性) も併せて検証する
   - `test_stream_error_then_delayed_data_reports_discarded`: 攻撃シナリオの経路遷移 (1 回目の違反 DATA で `Event::StreamReset`、同一ストリーム ID への遅延 DATA で `Event::DataDiscarded` が通知されること)
   - `test_data_discarded_after_normal_close`: 通常クローズ (END_STREAM 受信で `streams` から削除済み) 後の遅延 DATA で `Event::DataDiscarded` が通知されること
   - `test_data_discarded_on_closed_stream_in_map`: `process_headers` のエラー経路 (1xx + END_STREAM malformed) でマップ内に Closed 状態のストリームが残り、そのストリームへの遅延 DATA で `Event::DataDiscarded` が通知されること (RST_STREAM 送信・`Event::StreamReset` 再発の非発生を含む)
   - `test_empty_data_discarded_no_event`: 空 DATA の破棄で `Event::DataDiscarded` が生成されないこと
   - `test_padded_data_discarded_counts_padding`: パディング付き遅延 DATA の破棄でペイロード全体 (1 + 2 + 5 = 8) が通知されること
   - `test_padded_violation_data_counts_padding_in_stream_reset`: パディング付き違反 DATA の内部リセットでペイロード全体 (1 + 6 + 5 = 12) が通知されること
   - `test_padding_only_data_discarded_notifies_consumed`: パディングのみ DATA (1 + 0 + 5 = 6) の破棄で通知されること
   - `test_peer_rst_stream_pushes_event`: ピア RST_STREAM 受信で `Event::StreamReset` が `connection_window_consumed: 0` で通知されること
   - `test_connection_window_exhaustion_without_replenishment`: 補充しない場合に接続ウィンドウが枯渇して FLOW_CONTROL_ERROR の接続エラーになること (4 回目で枯渇)
   - `test_connection_window_replenishment_keeps_connection`: 補充した場合に接続が維持され、正当なストリーム (ID 3) の DATA 受信が継続できること
   - ヘルパー: `receive_content_length_request` (Content-Length: 5 リクエスト受信と既存イベント消費を共通化)
7. `CHANGES.md` の `## develop` に `[CHANGE]` エントリを追加した
8. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ることを確認した

## 参照

- `refs/rfc9113.txt` — Section 6.9 (Flow Control) / Section 5.1 (Stream States) / Section 5.4.1 (Connection Errors) / Section 5.4.2 (Stream Errors) / Section 8.1.1 (Malformed Messages)
- `src/connection.rs` — `Connection::handle_data` / `Connection::reset_stream`
- `src/flow_control.rs` — `FlowControl::consume_recv` / `FlowControl::should_send_window_update` / `FlowControl::window_update_increment`
- `src/event.rs` — `Event::StreamReset` (バイト数フィールドなし)
- `crates/tokio-http2/src/webtransport.rs` — `Event::DataReceived` に応じた WINDOW_UPDATE 送信の先例
- `issues/closed/0101-bug-fix-handle-data-stream-error.md` — ストリームエラーの RST_STREAM 変換と接続維持の先行対応 (遅延 DATA で `Event::StreamReset` が再発しないテストを含む)
- `issues/0103-bug-fix-no-content-empty-data.md` — 同じ `handle_data` の no-content 分岐を変更するため、実装順序の調整が必要

## 残課題 (本 issue のスコープ外)

- 正常経路の補充は `Event::DataReceived` の `data.len()` 基準 (パディング除外) であるのに対し、本 issue の通知は `flow_control_size` (パディング込み) 基準。パディング付き DATA では正常経路でも `1 + pad` バイトずつウィンドウが残留する既存の不整合があるが、本 issue では扱わない (別途対応)
- `FlowControl::should_send_window_update` / `FlowControl::window_update_increment` (`src/flow_control.rs`) は現状 `src/connection.rs` から一切呼ばれていない。アプリ側の補充判断基準として将来使用する想定か、デッドコードとして削除するかは別途判断する
