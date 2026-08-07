# 内部 RST_STREAM 送信時に Event::StreamReset が通知されずストリーム状態が残留する問題を修正する

- Priority: Medium
- Created: 2026-08-07
- Polished: 2026-08-07
- Model: deepseek-v4-flash
- Branch: feature/fix-internal-reset-stream-event

## 目的

`Connection::reset_stream` がライブラリ内部 (ストリームエラー処理) から呼ばれた場合に `Event::StreamReset` が push されない。`Event::StreamReset` / `Event::StreamClosed` はリセット・クローズ時に利用者がストリームの終了を認識するための終了イベントであり、通知が無いとサーバー側でストリーム単位の状態 (一時ファイル等) が接続終了まで残留する。内部リセット時にも RST_STREAM 受信パスと対称に `Event::StreamReset` を通知する。

対象は `Connection::reset_stream` を呼ぶ 3 経路 (デコードエラー / フロー制御違反 / ウィンドウオーバーフロー) のみであり、`reset_stream` を呼ばずに `Err(stream_error)` を返すストリームエラー経路 (Content-Length 不一致、no-content 違反、ヘッダー検証エラー等) は対象外とする。

## 現状

- `Event::StreamReset` は `src/connection.rs` の `handle_rst_stream` (ピアから RST_STREAM フレームを受信したとき) でのみ push される。このとき同時に `closed_streams` への登録と `streams` からの削除も行われる
- 一方 `Connection::reset_stream` (送信パス) は RST_STREAM フレームの送信と `send_rst_stream` による状態遷移 (Closed) のみを行い、`Event::StreamReset` の push・`closed_streams` への登録・`streams` からの削除のいずれも行わない
- `Connection::reset_stream` の内部呼び出し箇所は以下の 3 箇所:
  - `Connection::process` — フレームデコード時のストリームエラー (RFC 9113 Section 5.4.2)
  - `Connection::handle_data` — DATA フレームのストリームレベルフロー制御違反
  - `Connection::handle_window_update` — ストリームレベルのウィンドウオーバーフロー
- このため、ライブラリ内部でリセットされたストリームは利用者に `Event::StreamReset` を通知されず、`streams` に Closed 状態のまま残存する。`try_remove_closed_stream` は送信系パスと送信完了後の削除処理 (`Connection::handle_window_update`) でのみ呼ばれるため、送信バッファが空のストリームは典型的には接続終了まで残る
- なお、送信バッファに残データがあるリセット済みストリームは、次の接続レベル WINDOW_UPDATE 受信時に Closed 状態のまま DATA が送信され (RFC 9113 Section 5.4.2 の「RST_STREAM はそのストリームに送信できる最後のフレーム」違反)、その後 `try_remove_closed_stream` が `Event::StreamClosed` を通知して削除する既存バグがある。送信待ち END_STREAM が残っている場合は `complete_send_data` が Closed 状態でエラーを返し、`process` 自体がエラーになる。本修正で `streams` から即時削除されるため、いずれのケースも解消される

根拠: canary 版を利用するサーバー側のレビューで「ストリームエラー時に内部で reset_stream を呼んでも Event::StreamReset を push しないため、サーバー側でストリーム状態 (一時ファイル) が接続終了まで残留する」と報告された。

## 設計方針

`Connection::reset_stream` 内で受信パス (`handle_rst_stream`) と対称の処理を行う。以下 1〜3 はすべて、RST_STREAM 送信 (`send_frame`) の成功後、かつ `streams` にストリームが存在する場合にのみ行う:

1. `Event::StreamReset` を push する (エラーコードは呼び出し元が指定した値)
2. `closed_streams` にストリーム ID を登録する
3. `streams` からストリームを削除する

既存の `send_rst_stream` による Closed 状態遷移と RST_STREAM フレーム送信は維持する。送信に失敗した場合は既存挙動のままエラーを返し、イベント push とクリーンアップは行わない。

`streams` に存在しないストリーム (クローズ済み・idle) への呼び出しでは既存挙動 (RST_STREAM 送信のみ) を維持する。無条件に `closed_streams` へ登録すると、以後同じストリーム ID で到着した正規の新規 HEADERS が破棄済みとして扱われてしまうため (`src/connection/headers.rs` の `is_previously_closed` 判定)、条件は必須である。

`reset_stream` は公開 API でもあるため、利用者による明示的なリセット呼び出しでも `Event::StreamReset` が通知されることになるが、`Event::StreamReset` の意味論 (ストリームがリセットされた) と受信パスの挙動に合致する。

ストリーム削除後、遅延到着フレームは `streams` に存在しないことにより、`check_not_idle_stream` / `handle_data` / `handle_window_update` / `handle_headers` でクローズ済みストリームと同等に処理される。ただし、`is_idle_stream` / `check_not_idle_stream` は `closed_streams` を考慮せず `last_recv_stream_id` で idle 判定するため、`last_recv_stream_id` を超えるリセット済みストリーム (クライアントが送信開始したストリームの明示リセット等) への遅延フレームは idle へのフレームとして接続エラーに昇格する。また、`handle_headers` の GOAWAY 送信後チェックも同様に `closed_streams` を考慮しないため、リセット済みストリームへの遅延レスポンス HEADERS が「新規ストリーム」として接続エラーに昇格する。この誤判定を防ぐため、本修正で `is_idle_stream` / `check_not_idle_stream` / GOAWAY 送信後チェックに `closed_streams` の参照を追加し、リセット済みストリーム (既存のクローズ済みストリーム・受信 RST_STREAM で削除されたストリームを含む) を idle・新規ストリームと判定しないようにする。

なお、リセット済みストリーム宛の遅延 WINDOW_UPDATE 受信時には `Event::WindowUpdateReceived` が通知されるようになる (修正前は Closed 状態で早期 return され通知されなかった)。これは受信 RST_STREAM で削除されたストリームと同じ既存の挙動であり、整合する。

## 完了条件

- `streams` に存在するストリームへの内部 `reset_stream` 呼び出し (デコードエラー / フロー制御違反 / ウィンドウオーバーフロー) の際に `Event::StreamReset` が push される
- `streams` に存在したリセットされたストリームが `closed_streams` に登録され、`streams` から削除される
- `reset_stream` を利用者が `streams` に存在するストリームへ明示的に呼んだ場合も `Event::StreamReset` が push される
- リセット済みストリーム (送信開始ストリームを含む) への遅延フレームが破棄され、idle 誤判定による接続エラーに昇格しない
- リセット済みストリームの送信バッファ残データが、接続レベル WINDOW_UPDATE 受信時に送信されない
- 上記を検証するテストが追加され、全テストが通過する

## 解決方法

1. `src/connection.rs` の `Connection::reset_stream` にイベント push とストリームクリーンアップを追加し、`is_idle_stream` / `check_not_idle_stream` / `src/connection/headers.rs` の GOAWAY 送信後チェックに `closed_streams` の参照を追加する (既存の `send_rst_stream` による Closed 遷移と RST_STREAM 送信は維持する)
2. テストを追加する (`tests/test_connection.rs` に単体テストとして追加する。いずれも意図的なエラーパスの検証であり、PBT (`pbt/`) ではなく単体テストで検証する):
   - ストリームレベルのフロー制御違反 (受信ウィンドウ超過の DATA) を送り、`Event::StreamReset` (FLOW_CONTROL_ERROR) が通知されることを検証する
   - デコードエラー (例: ストリーム向け WINDOW_UPDATE の増分 0。`WindowIncrement` 型は非ゼロを構造的に保証するため、テストでは raw バイト列でフレームを構築する) でストリームエラーが発生した場合に `Event::StreamReset` が通知されることを検証する
   - ストリーム向け WINDOW_UPDATE でウィンドウオーバーフローを発生させた場合に `Event::StreamReset` が通知されることを検証する
   - 利用者が `reset_stream` を明示的に呼んだ場合に `Event::StreamReset` が通知されることを検証する
   - 利用者がクローズ済み・idle ストリームへ `reset_stream` を明示的に呼んだ場合に `Event::StreamReset` が push されないことを検証する
   - クライアントが送信開始したストリーム (last_recv_stream_id 超過) を明示リセットし、その後同ストリームへ遅延 DATA を送信して破棄され、接続が維持されることで、`check_not_idle_stream` の `closed_streams` 考慮を検証する (遅延 HEADERS の破棄は `handle_headers` の `is_previously_closed` 判定で `closed_streams` への登録を検証する。`streams` からの削除は次のテスト項目で検証する。private フィールドにはアクセスできないため)
   - リセット済みストリームへストリーム向け WINDOW_UPDATE の増分 0 (デコードエラー) を送信しても、`is_idle_stream` による接続エラー昇格が起きず接続が維持されることを検証する (process のデコードエラー処理の `closed_streams` 考慮の検証。リセット済みストリームへの呼び出しでは `Event::StreamReset` は push されない)
   - GOAWAY 送信後にリセット済みストリームへ遅延レスポンス HEADERS を送信しても、GOAWAY 送信後チェックで接続エラーにならず破棄されることを検証する
   - リセット時に送信バッファに残データがある状態で接続レベル WINDOW_UPDATE を受信しても、出力に DATA フレームが現れないことを検証する (`streams` からの削除の検証)
   - 既存の `pbt/tests/prop_connection/main.rs` の `prop_rst_stream_cancels_stream` は受信パスの検証であり、変更しない
3. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する (shiguredo-changelog スキルを参照)
4. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` を実行する

## 参照

- `src/connection.rs` — `Connection::reset_stream` / `Connection::handle_rst_stream` / `Connection::process` / `Connection::handle_data` / `Connection::handle_window_update` / `is_idle_stream` / `check_not_idle_stream`
- `src/connection/headers.rs` — `last_recv_stream_id` の更新と `is_previously_closed` 判定
- `src/event.rs` — `Event::StreamReset`
- `pbt/tests/prop_connection/main.rs` — 受信パスの RST_STREAM テスト
- `issues/0100-bug-fix-reset-stream-idle-stream.md` — idle ストリームへの RST_STREAM 送信の禁止 (RFC 9113 Section 6.4) への対応。0099 の後に実装する
