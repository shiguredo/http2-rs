# WebTransport ドライバが大きな WT 送信を 1 回の HTTP/2 send_data で送り送信バッファ上限で失敗する

- Created: 2026-09-10
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-flush-output-split-large-send
- Polished: {YYYY-MM-DD}

## 目的

`crates/tokio-http2/src/webtransport.rs` の `DriverState::flush_wt_output` が WT 出力全体を 1 回の `Connection::send_data` で送るため、送信バッファの固定容量 (65535) を超える WT 送信がストリームエラーになり driver が終了する問題を修正する。送信バッファ容量がピアの `SETTINGS_INITIAL_WINDOW_SIZE` に固定されていた頃は、ピアが 65535 超の初期ウィンドウを広告すれば 1 回の大容量送信が滞留して成功していたため、これは後方互換のない退行である。

## 現状

`flush_wt_output` は `wt_session.poll_output()` が返す出力バッファ全体 (`Vec<u8>`) を 1 回の `conn.send_data(connect_stream_id, out, false)` で送る。`WtSession::poll_output` は出力バッファを全量 drain するため、アプリの 1 回の `WtBidiStream::send` / `WtUniSendStream::send` で 65535 bytes を超えるデータを渡すと、HTTP/2 層の送信バッファ容量 (固定 65535) を超えて `queue_data` がストリームエラーを返し、`send_cmd_result` が driver を終了する。

到達経路は公開 API で、`LimitsBuilder::initial_window_size` でクライアントが 65535 超の初期ウィンドウを広告できる。WebTransport のストリーム送信ウィンドウは既定 256KiB であり、WT 層は 1 回 256KiB まで許可するが HTTP/2 層が受け取れない非対称がある。

## 設計方針

- `flush_wt_output` で `out` を送信バッファ容量 (65535) 以下に分割して順次 `send_data` する (HTTP/2 はバイトストリームなので capsule 境界と無関係に分割してよい)
- tokio-http2 の `Connection::send_data` の doc に「1 回の呼び出し上限は 65535 bytes であり、それ以上は分割が必要」を明記する
- 既存テスト `test_wt_command_flush_error_not_masked_as_connection_closed` の失敗トリガーは「出力サイズ超過」ではなく別の送信不能状態に差し替える (分割後はサイズ超過で失敗しなくなるため)
- ピアが 65535 超の初期ウィンドウを広告する構成で、65535 bytes を超える WT 送信が成功するテストを追加する

## 完了条件

- ピアが 65535 超の初期ウィンドウを広告する構成で、65535 bytes を超える WT 送信が成功すること
- `flush_wt_output` が出力を分割して送信すること
- テストが追加され、`cargo test --all` が通過すること
