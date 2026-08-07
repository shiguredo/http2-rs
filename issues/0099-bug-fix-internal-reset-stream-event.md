# 内部 RST_STREAM 送信時に Event::StreamReset が通知されずストリーム状態が残留する問題を修正する

- Priority: Medium
- Created: 2026-08-07
- Polished: {YYYY-MM-DD}
- Model: deepseek-v4-flash
- Branch: feature/fix-internal-reset-stream-event

## 目的

`Connection::reset_stream` がライブラリ内部 (ストリームエラー処理) から呼ばれた場合に `Event::StreamReset` が push されない。`Event::StreamReset` / `Event::StreamClosed` は利用者がストリームの終了を認識する唯一の手段であるため、通知が無いとサーバー側でストリーム単位の状態 (一時ファイル等) が接続終了まで残留する。内部リセット時にも RST_STREAM 受信パスと対称に `Event::StreamReset` を通知する。

## 現状

- `Event::StreamReset` は `src/connection.rs` の `handle_rst_stream` (ピアから RST_STREAM フレームを受信したとき) でのみ push される。このとき同時に `closed_streams` への登録と `streams` からの削除も行われる
- 一方 `Connection::reset_stream` (送信パス) は RST_STREAM フレームの送信と `send_rst_stream` による状態遷移 (Closed) のみを行い、`Event::StreamReset` の push・`closed_streams` への登録・`streams` からの削除のいずれも行わない
- `Connection::reset_stream` の内部呼び出し箇所は以下の 3 箇所:
  - `Connection::process` — フレームデコード時のストリームエラー (RFC 9113 Section 5.4.2)
  - `Connection::handle_data` — DATA フレームのストリームレベルフロー制御違反
  - `Connection::handle_window_update` — ストリームレベルのウィンドウオーバーフロー
- このため、ライブラリ内部でリセットされたストリームは利用者にどの終了イベントも通知されず、`streams` に Closed 状態のまま残存する (`try_remove_closed_stream` は送信系パスのみで呼ばれるため、典型的には接続終了まで残る)

根拠: canary 版を利用するサーバー側のレビューで「ストリームエラー時に内部で reset_stream を呼んでも Event::StreamReset を push しないため、サーバー側でストリーム状態 (一時ファイル) が接続終了まで残留する」と報告された。

## 設計方針

`Connection::reset_stream` 内で受信パス (`handle_rst_stream`) と対称の処理を行う:

1. `streams` にストリームが存在する場合、`Event::StreamReset` を push する (エラーコードは呼び出し元が指定した値)
2. `closed_streams` にストリーム ID を登録する
3. `streams` からストリームを削除する

`reset_stream` は公開 API でもあるため、利用者による明示的なリセット呼び出しでも `Event::StreamReset` が通知されることになるが、`Event::StreamReset` の意味論 (ストリームがリセットされた) と受信パスの挙動に合致する。ストリーム削除後も、遅延到着フレームは `closed_streams` / `streams` に存在しないことにより既存のクローズ済みストリームと同等に処理される。

## 完了条件

- ストリームエラーによる内部 `reset_stream` 呼び出し (デコードエラー / フロー制御違反 / ウィンドウオーバーフロー) の際に `Event::StreamReset` が push される
- リセットされたストリームが `closed_streams` に登録され、`streams` から削除される
- `reset_stream` を利用者が明示的に呼んだ場合も `Event::StreamReset` が push される
- 上記を検証するテストが追加され、全テストが通過する

## 解決方法

1. `src/connection.rs` の `Connection::reset_stream` にイベント push とストリームクリーンアップを追加する
2. テストを追加する:
   - ストリームレベルのフロー制御違反 (受信ウィンドウ超過の DATA) をサーバーに送り、`Event::StreamReset` (FLOW_CONTROL_ERROR) が通知されることを検証する (既存の `pbt/tests/prop_connection/main.rs` の `prop_rst_stream_cancels_stream` が受信パスの検証なので、内部リセット側のテストを新規に追加する)
   - デコードエラーでストリームエラーが発生した場合に `Event::StreamReset` が通知されることを検証する
3. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する
4. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` を実行する

## 参照

- `src/connection.rs` — `Connection::reset_stream` / `Connection::handle_rst_stream` / `Connection::process` / `Connection::handle_data` / `Connection::handle_window_update`
- `src/event.rs` — `Event::StreamReset`
- `pbt/tests/prop_connection/main.rs` — 受信パスの RST_STREAM テスト
