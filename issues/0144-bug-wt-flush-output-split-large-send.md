# WebTransport ドライバが大きな WT 送信を 1 回の HTTP/2 send_data で送り送信バッファ上限で失敗する

- Created: 2026-09-10
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-flush-output-split-large-send
- Polished: 2026-09-10

## 目的

`crates/tokio-http2/src/webtransport.rs` の `DriverState::flush_wt_output` が WT 出力全体を 1 回の `Connection::send_data` で送るため、送信バッファの固定容量 (65535) を超える WT 送信がストリームエラーになり driver が終了する問題を修正する。送信バッファ容量がピアの `SETTINGS_INITIAL_WINDOW_SIZE` に固定されていた頃は、ピアが 65535 超の初期ウィンドウを広告すれば 1 回の大容量送信が滞留して成功していたため、これは後方互換のない退行である。

## 現状

`flush_wt_output` は `wt_session.poll_output()` が返す出力バッファ全体 (`Vec<u8>`) を 1 回の `conn.send_data(connect_stream_id, out, false)` で送る。`WtSession::poll_output` は出力バッファを全量 drain するため、アプリの 1 回の `WtBidiStream::send` / `WtUniSendStream::send` で 65535 bytes を超えるデータを渡すと、HTTP/2 層の送信バッファ容量 (固定 65535) を超えて `queue_data` がストリームエラーを返し、`send_cmd_result` が driver を終了する。

到達経路は公開 API で、`LimitsBuilder::initial_window_size` でクライアントがストリームレベルの初期ウィンドウを 65535 超に広告できる。WebTransport のストリーム送信ウィンドウは既定 256KiB であり、WT 層は 1 回 256KiB まで許可するが HTTP/2 層が受け取れない非対称がある。

なお HTTP/2 の接続レベル送信ウィンドウは `SETTINGS_INITIAL_WINDOW_SIZE` では変化せず、`LimitsBuilder::connection_window_size` を 65535 超に設定して接続レベル WINDOW_UPDATE を送るか、ピアが WINDOW_UPDATE を返さないと拡張されない (RFC 9113 Section 6.9.2)。分割後も接続ウィンドウが枯渇すれば後続チャンクが送信バッファに滞留し、累積で容量 65535 を超えると `send buffer full` になる。

## 設計方針

- `flush_wt_output` で `out` を送信バッファ容量 (65535) 以下に分割して順次 `send_data` する (HTTP/2 はバイトストリームなので capsule 境界と無関係に分割してよい)
- `flush_wt_output` が実際に呼ぶ tokio-http2 の `ServerConnection::send_data` (および委譲先の `tokio_http2::Connection::send_data`) の doc に「1 回の呼び出し上限は 65535 bytes であり、それ以上は分割が必要」を明記する。sans-io の `Connection::send_data` は既に同内容を記載済み
- `send_cmd_result` の doc はフラッシュ失敗を「収まらない出力を一度に送ろうとした状態」「全量拒否のため capsule は積まれない」と説明しているが、分割後は途中チャンクまで送信済みで後続チャンクが累積バッファ超過になる場合がある。失敗原因の説明を分割後の挙動に合わせて修正する
- 既存テスト `test_wt_command_flush_error_not_masked_as_connection_closed` はクライアントが `Limits::default()` (接続・ストリームウィンドウ各 65535) で WINDOW_UPDATE を返さないため、分割後も接続ウィンドウ枯渇による累積バッファ超過で同じ `send buffer full` になる。トリガー差し替えは不要であり、失敗原因が「1 回のサイズ超過」ではなく「接続ウィンドウ枯渇による累積バッファ超過」である点にコメントを修正する
- ピアが 65535 超の初期ウィンドウを広告する構成で、65535 bytes を超える WT 送信が成功するテストを追加する。接続レベル送信ウィンドウは `SETTINGS` では拡張されないため、クライアントの `Limits` で `initial_window_size` と `connection_window_size` の両方を送信データ量以上に設定する。WT ストリーム送信で検証する場合は、クライアントが広告する `wt_initial_max_stream_data_*` / `wt_initial_max_data` も送信データ量以上にする (DATAGRAM 送信は WT フロー制御を消費しないため不要)

## 完了条件

- ピアが 65535 超の初期ウィンドウ (`initial_window_size` と `connection_window_size` の両方) を広告する構成で、65535 bytes を超える WT 送信が成功すること
- `flush_wt_output` が出力を分割して送信すること
- テストが追加され、`cargo test --all` が通過すること
