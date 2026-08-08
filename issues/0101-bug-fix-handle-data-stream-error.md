# handle_data 内のストリームエラーで RST_STREAM を送信せず接続が終了する問題を修正する

- Priority: High
- Created: 2026-08-08
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-handle-data-stream-error
- Polished: 2026-08-08

## 目的

`Connection::handle_data` が `Err(Error::stream_error(...))` を返す 4 経路 (no-content 違反 / Content-Length 超過 / END_STREAM 時不一致 / 状態遷移違反) は、`Connection::process` の `self.handle_frame(frame)?` を経由してそのまま `Err` として呼び出し元に伝播する。`Error` 型には stream_id が含まれないため、呼び出し側はどのストリームをリセットすべきかを特定できず、接続エラーとして扱わざるを得ない (kikyo-local では GOAWAY (PROTOCOL_ERROR) 送信 + 接続終了になるという観察報告がある)。

RFC 9113 Section 8.1.1 は「Malformed requests or responses that are detected MUST be treated as a stream error (Section 5.4.2) of type PROTOCOL_ERROR」と規定しており、Content-Length 不一致はストリームエラーとして RST_STREAM で処理し、接続を維持しなければならない。同様に RFC 9113 Section 5.1 は half-closed (remote) 状態のストリームへの DATA 受信を STREAM_CLOSED のストリームエラーで処理することを MUST と規定している。

## 現状

- `src/connection.rs` の `Connection::handle_data` は到達可能な 4 箇所で `Err(Error::stream_error(...))` を返す:
  - no-content 違反 (204/304/HEAD への DATA) — `ErrorCode::ProtocolError`
  - Content-Length 超過 — `ErrorCode::ProtocolError`
  - END_STREAM 時不一致 — `ErrorCode::ProtocolError`
  - `stream.state_machine_mut().recv_data(frame.end_stream)?` の状態遷移違反 (HalfClosedRemote 状態への DATA) — `ErrorCode::StreamClosed`
- `Connection::process` はフレームデコードエラー経路でのみ `e.is_stream_error()` を捕捉して `last_decoded_stream_id` で `reset_stream` を呼ぶ。`handle_frame` 内で発生したストリームエラーは捕捉されず `Err` として伝播する
- フロー制御違反は既に `Connection::handle_data` 内でエラー検出箇所から直接 `reset_stream` を呼び RST_STREAM に変換している (RFC 9113 Section 6.9 / Section 7 (Error Codes) の FLOW_CONTROL_ERROR 定義に基づく実装判断)
- 0099 で「`reset_stream` を呼ばずに `Err(stream_error)` を返すストリームエラー経路 (Content-Length 不一致、no-content 違反、ヘッダー検証エラー等) は対象外」と明記され、本問題は未対応のまま残っている

## 設計方針

フロー制御違反と同じパターンで、`Connection::handle_data` 内のエラー検出箇所から直接 `reset_stream` を呼び RST_STREAM 送信に変換する。対象は上記 4 経路のみとし、ヘッダー検証エラー (`src/connection/headers.rs` 内で発生するストリームエラー) は `handle_data` 外の経路のため本 issue では対象外とする (参照欄に残課題として記録する)。

- `Event::StreamReset` は 0099 の修正により `reset_stream` 内で push されるため、呼び出し側はストリーム終了 (一時ファイルの破棄) を認識できる。なお `Event::StreamClosed` は push されない (0099 の `reset_stream` の挙動)
- 既存のフロー制御違反処理 (エラー検出箇所で `self.reset_stream(...)?; return Ok(())`) と同一のパターンで書く。NLL により `stream` への可変借用は最後の使用箇所で終了するため、ブロック内で `self.reset_stream` を直接呼べる (src/connection.rs のフロー制御違反処理が証明済み)
- リセット時はその DATA のデータをイベントとして通知しない (フロー制御違反と同じ挙動)
- no-content 違反の扱いは現行の判定 (空 DATA も含めて違反とする) を継承し、変更しない。なお `src/connection/headers.rs` のコメントは「空 DATA は許容される」と書いているが、実装 (`Connection::handle_data` の no-content チェック) はデータ長を判定せず空 DATA も違反とする。このコメントと実装の不一致の解消 (コメント修正) は本 issue の範囲外とする

## 完了条件

- 上記 4 経路のいずれでも、ストリームエラーが `RST_STREAM` (該当するエラーコード: PROTOCOL_ERROR / STREAM_CLOSED) 送信 + `Event::StreamReset` push に変換され、接続が維持されること
- 上記のストリームエラーが `Connection::process` の `Err` として呼び出し元に伝播しないこと
- リセット後の遅延 DATA が破棄され、接続が維持されること
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ること

## 解決方法

1. `src/connection.rs` の `Connection::handle_data` 内の 4 箇所の `Err(Error::stream_error(...))` を、フロー制御違反処理と同じパターン (`self.reset_stream(...)?; return Ok(())`) に置き換える
   - no-content 違反 / Content-Length 超過 / END_STREAM 時不一致: `reset_stream` を `ErrorCode::ProtocolError` で呼ぶ
   - `recv_data` の状態遷移違反: `recv_data` のエラーを `.is_err()` で捕捉し、`reset_stream` を `ErrorCode::StreamClosed` で呼ぶ
   - `Event::DataReceived` は push しない
2. `tests/test_connection.rs` に単体テストを追加する (0099 の `mod reset_stream` と同じ構成。意図的なエラーパスの検証のため PBT ではなく単体テストで検証する):
   - Content-Length 超過の DATA 送信で RST_STREAM (PROTOCOL_ERROR) が出力され、`Event::StreamReset` が push され、接続が維持されること (サーバーロール。リクエストに Content-Length を付けて送信)
   - END_STREAM 時不一致で同様の挙動になること (サーバーロール)
   - no-content 違反 (204/304/HEAD) で同様の挙動になること (クライアントロール。`no_content` は `Role::Client` のレスポンス受信時のみ設定されるため)
   - HalfClosedRemote 状態のストリームへの DATA で RST_STREAM (STREAM_CLOSED) が出力され、`Event::StreamReset` が push され、接続が維持されること (サーバーロール。END_STREAM 付きリクエスト受信後に DATA を送信)
   - 上記各ケースで、リセットされた DATA が `Event::DataReceived` にならないこと (0099 の `test_flow_control_violation_pushes_stream_reset` と同様の検証)
   - リセット後の遅延 DATA が破棄され、接続が維持されること
3. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する (shiguredo-changelog スキルを参照)

## 参照

- `refs/rfc9113.txt` — Section 8.1.1 (Malformed Messages) / Section 5.4.2 (Stream Errors) / Section 5.1 (Stream States) / Section 6.9 (Flow Control) / Section 7 (Error Codes)
- `refs/rfc9110.txt` — Section 6.4.1 (Content) / Section 9.3.2 (HEAD) / Section 15.3.5 (204) / Section 15.4.5 (304)
- `src/connection.rs` — `Connection::handle_data` / `Connection::process` / `Connection::reset_stream`
- `src/stream/state.rs` — `StreamState::recv_data` (HalfClosedRemote 状態の DATA 受信エラー)
- `src/error.rs` — `Error` (stream_id を含まない)
- `issues/closed/0099-bug-fix-internal-reset-stream-event.md` — 本 issue の経路を対象外とした issue
- 残課題: `src/connection/headers.rs` のヘッダー検証エラー経路と、`Connection::handle_frame` の未知フレーム処理 (CONNECT 確立済みストリームへの unknown frame) も同様に `Err(stream_error)` を返して接続終了を引き起こすが、本 issue では対象外とする (別 issue で対応)
