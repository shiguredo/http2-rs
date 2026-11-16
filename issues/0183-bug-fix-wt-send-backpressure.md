# 大容量 WT 送信と HTTP/2 送信ウィンドウ枯渇で driver が接続ごと終了する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-send-backpressure
- Polished: 2026-11-17

## 目的

`tokio-http2` の WebTransport driver は、WT 出力を 65,535 バイトずつ `Connection::send_data` に渡すが、flush 中に WINDOW_UPDATE を読まない。WT 出力が HTTP/2 の connection / stream 送信ウィンドウと固定送信バッファを超えると `send buffer full` になり、driver が接続ごと終了する。既定設定で 200 KB の DATAGRAM を送るテストが、この接続終了を期待値として固定している。未送信出力を保持して送信バッファの空き回復後に送り、driver を終了させないようにする。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `flush_wt_output` は `WtSession::poll_output` の全出力を 65,535 バイトずつ `send_data` する。`send_data` は送信ウィンドウが不足すると内部バッファに積むが、バッファ容量を超えると `src/connection.rs` の `queue_data` が `ErrorCode::FlowControlError` のストリームエラー (`send buffer full`) を返す。このエラーは接続全体のエラーではなく、接続は引き続き使用できる (0137)。

`DriverState::send_cmd_result` は flush エラーで ack にエラーを載せ、`false` を返して driver を終了する。`DriverState::handle_event` の `Event::DataReceived` 分岐も `self.flush_wt_output().await?` で同じエラーを `run` まで伝播させ、driver を終了する。

`crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_command_flush_error_not_masked_as_connection_closed` は、クライアントを `Limits::default()` で接続し、`perform_connect` の後は `next_event` を呼ばず `send_window_update` も送らない (コメント「WINDOW_UPDATE を送らないままサーバーの処理完了を待つ」)。200 KB DATAGRAM の送信で `send buffer full` を期待しており、この挙動を固定している。クライアントの HTTP/2 受信ウィンドウは 65,535 のままなので、driver が出力を保持しても、クライアントが WINDOW_UPDATE を送らない限り 200 KB は送信し切れない。

## 設計方針

closed 0144 は 1 回の `send_data` が送信バッファ上限を超える問題をチャンク分割で修正したが、分割後のチャンクが送信ウィンドウ枯渇で累積する経路は未解決である。本 issue はその残存経路を対象とする。

- 未送信の WT 出力を `DriverState` の FIFO キューに保持する。`flush_wt_output` は `poll_output` の出力をキューに追加し、キュー先頭から 65,535 バイト以下のチャンクを `send_data` に渡す。`send buffer full` (`Error::Protocol` の内側の `ErrorCode::FlowControlError` かつ reason `send buffer full`) を受けたら、失敗したチャンクをキューの先頭に戻し、後続のチャンクも保持したまま、driver を終了せずに次の送信機会を待つ。それ以外のエラー (I/O、接続エラー、ストリーム状態) は従来どおり ack / `run` に伝える。
- キューのドレインは、コマンド処理後、`Event::WindowUpdateReceived` と `Event::SettingsReceived` の処理後、および既存の `Event::DataReceived` 分岐のフラッシュ時に試みる。`SettingsReceived` では `SETTINGS_INITIAL_WINDOW_SIZE` の増加でストリーム送信ウィンドウが広がり、滞留 DATA が送信されて送信バッファに空きができる経路がある (0135 / RFC 9113 Section 6.9.2)。WINDOW_UPDATE だけを契機にすると、SETTINGS で回復した場合にキューが停滞する。
- `DriverState::run` の `tokio::select!` は `biased` でコマンドを優先しているため、コマンドが継続して到着すると `WindowUpdateReceived` / `SettingsReceived` が処理されず、キューが停滞し得る。コマンド処理の後にイベントを処理する、または `biased` を外してイベントも公平に処理するようにする。
- `send buffer full` はバックプレッシャーとして扱い、WT セッションが操作を受け付けた場合は呼び出し側へ Ok を返す。HTTP/2 送信バッファへの滞留は 0138 と同じ「受け付けたが未送信」の状態であり、キューのドレインで後から送信する。呼び出し側が同じ操作を再試行すると二重送信になるため、flush の途中失敗をエラーとして返さない。
- WT 層が Close 以外の操作を受け付けなかった場合 (WT 送信ウィンドウ超過、1 capsule 16 MiB 超過、ストリーム状態違反) は、従来どおりエラーを返し、driver は継続する。WT 送信ウィンドウ超過の場合は WT_MAX_DATA / WT_MAX_STREAM_DATA の受信後に再試行できる。1 capsule の上限超過の場合は上限以下のデータを渡す必要がある。DATAGRAM は WT フロー制御の対象外であり、1 つの DATAGRAM は分割できないため、上限を超えるデータは送信 API がエラーを返す。呼び出し側は上限以下の DATAGRAM を送るか、アプリケーションの単位で複数の DATAGRAM に分ける。
- Close は 0138 の決定どおり終端操作とする。flush 後に未送信出力が残る場合は Err を返して driver を終了する。本 issue の「driver を終了しない」は Close 以外の送信経路に限る。
- キューは FIFO で上限を設けない。ストリームデータは WT 送信ウィンドウで有界であり、DATAGRAM は複数コマンド分が蓄積し得るが、既存の unbounded チャネルと同じ扱いとする。
- `WtBidiStream::send` / `WtUniSendStream::send` / `send_datagram` の rustdoc に、次の最大サイズと分割責任を明記する。
  - `send`: 1 回に渡せるのは、ピアが広告した WT 送信ウィンドウ (セッションの `initial_max_data` とストリームの `initial_max_stream_data_*`、WT_MAX_DATA / WT_MAX_STREAM_DATA で更新) の残量と、0182 で導入する 1 capsule のエンコード後サイズ 16 MiB 上限の両方を満たす範囲。0182 が先に実装される前提とする。超える場合は呼び出し側が複数の `send` に分割する。
  - `send_datagram`: WT フロー制御の対象外で、0182 の 1 capsule 16 MiB 上限から DATAGRAM capsule のヘッダー (type 1 バイト + length 4 バイト) を引いたサイズが最大。呼び出し側は 1 つの DATAGRAM を分割できず、driver が capsule を HTTP/2 DATA フレームに分割して送信する。
  - `send` / `send_datagram` の Ok は「WT セッションが受け付けた」ことを示し、HTTP/2 送信バッファや driver キューに滞留して実際の送信が遅延し得ること、セッション drop 時に未送信出力が失われ得ることを明記する。
- `send_cmd_result` / `flush_wt_output` の doc を、バックプレッシャー時に driver を終了しない新しい挙動に合わせて更新する。
- 既定設定で 200 KB を送るテストを「接続終了」ではなく「バックプレッシャー後に送信完了」の期待値に変更する。クライアントは `next_event` を駆動し、CONNECT ストリームの `DataReceived` ごとに connection と stream の `send_window_update` を送り、200 KB の DATAGRAM capsule を受信し切るまでサーバー側はセッションを保持する。単に期待値を Ok に変えるだけでは、クライアントが WINDOW_UPDATE を送らないため完了しない。テストにはタイムアウトを付け、WINDOW_UPDATE が送られない場合にハングせず失敗させる。
- ウィンドウ枯渇と回復の統合テストを追加する。DATAGRAM 200 KB のテストに加え、ピアの WT 送信ウィンドウ (`initial_max_data` / `initial_max_stream_data_bidi_remote`) を 200 KB 以上にし、`initial_max_streams_bidi` を 1 以上に広告した構成で 200 KB の WT ストリームデータを送り、HTTP/2 の既定ウィンドウ枯渇後に WINDOW_UPDATE で再開することを検証する。ピアの WT 送信ウィンドウが不足する場合は `send` が事前にエラーを返し、driver が継続することを検証する。コマンドが継続して到着する状況でも WINDOW_UPDATE が処理されることを確認する。
- `handle_event` 経由のバックプレッシャーも検証する。クライアントが WT 設定を広告する Limits を使い、200 KB DATAGRAM の送信中に CONNECT ストリームへ小さな WT_STREAM capsule を送り、サーバーが `Event::DataReceived` で WT 出力を生成する状況で driver が終了しないことを確認する。

## 完了条件

- HTTP/2 の送信ウィンドウが不足しても driver が終了せず、WINDOW_UPDATE または SETTINGS_INITIAL_WINDOW_SIZE の増加後にキュー済みの WT 出力が送信されること。
- コマンドが継続して到着しても WINDOW_UPDATE / SETTINGS が処理され、キューが停滞しないこと。
- `handle_event` の `Event::DataReceived` 分岐で flush がバックプレッシャーになっても driver が終了しないこと。
- 既定設定 (HTTP/2 初期ウィンドウ 65,535) で 200 KB の DATAGRAM が driver 終了せず送信完了すること。クライアントが `next_event` を駆動して WINDOW_UPDATE を送るテストで検証すること。
- ピアの WT 送信ウィンドウが 200 KB 以上ある構成で、200 KB の WT ストリームデータが HTTP/2 WINDOW_UPDATE 後に送信完了すること。WT 送信ウィンドウが不足する場合は `send` が事前にエラーを返し、driver が継続すること。
- `WtBidiStream::send` / `WtUniSendStream::send` / `send_datagram` の rustdoc に、最大サイズ、分割責任、Ok の意味が明記されていること。`send_datagram` の最大サイズは 0182 の 1 capsule 16 MiB 上限と整合すること。
- `send_cmd_result` / `flush_wt_output` の doc が新しい挙動に合っていること。
- バックプレッシャーの回帰テストが追加され、既存の `test_wt_command_flush_error_not_masked_as_connection_closed` が新しい期待値に更新されていること。テストはタイムアウトでハングを防ぐこと。
- Close の終端挙動 (0138) が維持されていること。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
