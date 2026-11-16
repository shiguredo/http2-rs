# SETTINGS ACK 前の CONNECT で WebTransport 初期 credit が 0 のまま停止する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-initial-credit-ack-race
- Polished: 2026-11-17

## 目的

`WtServerRequest::accept` は ACK 済みの自広告 SETTINGS からローカル初期値を決める。クライアントがサーバー SETTINGS を ACK する前に CONNECT を送ると、サーバーのローカル初期値がすべて 0 になり、driver の自動拡張も `initial == 0` で停止する。さらに `WtServerSession` / `WtSessionHandle` には WT_MAX_DATA / WT_MAX_STREAMS / WT_MAX_STREAM_DATA を送る API がないため、クライアントからサーバー方向のストリームデータが流せないまま停止する。draft-ietf-webtrans-http2-16 Section 4.3.1 / Section 11.2 に沿って、2xx 応答後に初期 credit を capsule で回復する経路を用意する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept` は `config.apply_advertised_settings(conn.acked_settings())` でアプリ指定の `WtConfig` を ACK 済み SETTINGS のみに置き換える。ACK がまだ処理されていない場合、全項目が 0 になる。

ACK が後から処理されても、`WtSession` のローカル config は accept 時点の値のまま更新されない。そのため、初期 capsule を送って `flow_control.recv_max` などを増やしても、`maybe_grow_session_window` / `maybe_grow_stream_window` / `maybe_grow_max_streams` が参照する `config().initial_*` は 0 のままで、自動拡張は再開しない。

`DriverState::maybe_grow_session_window` は `should_send_max_data(0)` が常に false、`maybe_grow_max_streams` は `initial == 0` で即 `Ok(())`、`maybe_grow_stream_window` も初期値 0 では発火しない。`WtServerSession` / `WtSessionHandle` には `send_max_data` / `send_max_streams` / `send_max_stream_data` がない。

draft-16 Section 4.3.1 は SETTINGS と ACK が in-flight である場合を明示的に扱い、ACK 前は Initial Value 0 を使う。Section 11.2 は初期値 0 の場合、ピアがデータを送る前に WT_MAX_DATA を送る必要があり、ストリームデータについては WT_MAX_STREAM_DATA も送る必要があるとしている。クライアントが SETTINGS を処理する前に CONNECT を送ることは禁止されていない。

## 設計方針

- 初期値が 0 の項目に限り、driver が 2xx 応答後に WT_MAX_DATA / WT_MAX_STREAMS（bidi / uni の両方向）を送る経路を追加する。driver は起動直後に回復 capsule をフラッシュする。初期値が非 0 の項目は自動送信を行わない。同じ値を送ると Section 6.5 / Section 6.7 の非増加規則に違反するためであり、増加が必要な場合はアプリが追加 API で送る。
- WT_MAX_STREAM_DATA は、ストリームの初期受信上限が 0 の場合に限り、ピア開始ストリームの `WtEvent::StreamOpened` 時と、サーバーがローカル開始 bidi ストリームを `open_bidi` で開いた時に、ストリームが `Recv` 状態のときだけ送る。初期受信上限が非 0 の場合は同じ値を送ると Section 6.6 の非増加規則に違反するため送らない。ストリームが存在しないと送れないため、2xx 直後ではなくストリーム生成時に行う。値は、ピア開始 bidi は `initial_max_stream_data_bidi_remote`、ローカル開始 bidi は `initial_max_stream_data_bidi_local`、ピア開始 uni は `initial_max_stream_data_uni` をそれぞれ使う。
- 送る値は、`accept` に渡された `WtConfig`（上書き前）の値（アプリ指定の受信上限）を第一候補とし、0 の項目は `conn.local_settings()` の自広告値を使う。両方 0 の項目は capsule を送らない。
- driver は回復に使った値を保持し、`config().initial_*` が 0 の間は `maybe_grow_session_window` / `maybe_grow_stream_window` / `maybe_grow_max_streams` のしきい値と増分にその値を使う。これにより初期ウィンドウを使い切った後も自動拡張が継続する。
- `WtServerSession` / `WtSessionHandle` に `send_max_data` / `send_max_streams` / `send_max_stream_data` を追加する。driver へのコマンドと oneshot ack で実装し、`WtSession` の同名 API に委譲する。エラーは `Error::WebTransport` で返す。
- 0 初期値のセッションを拒否する方針は採らない。2xx を返した後に capsule で回復する。`accept` の doc に、ACK 未処理時の初期値 0 と回復動作を明記する。
- SETTINGS ACK を CONNECT より後に送るクライアントを使った統合テストを追加する。`Client::connect` の直後、`SettingsReceived` を待たずに `send_request` で CONNECT を送り、wire 上で CONNECT を ACK より先にする。既存の `perform_connect` は必ず SETTINGS 受信後に CONNECT を送るため、この経路を通らない。

## 完了条件

- SETTINGS ACK が CONNECT より後になっても、2xx 後に WT_MAX_DATA / WT_MAX_STREAMS（bidi / uni の両方向）が送られ、ピアが空 WT_STREAM でストリームを開いて WT_MAX_STREAM_DATA を受け取り、ストリームデータを送れること。
- サーバーが `open_bidi` で開いた bidi ストリームでも、ピアが WT_MAX_STREAM_DATA を受け取り、ストリームデータを送れること。
- 初期ウィンドウを使い切った後も driver の自動拡張が継続し、ピアが継続してストリームデータを送れること。
- フロー制御 capsule の送信 API が `WtServerSession` / `WtSessionHandle` に追加され、統合テストで使用され、成功とエラー（非増加など）が検証されていること。
- ACK 順序を入れ替えた統合テストが追加されていること。
- ACK 済みで初期値が非 0 の通常経路では、accept 直後の WT_MAX_DATA / WT_MAX_STREAMS の自動送信と、ストリーム生成時の WT_MAX_STREAM_DATA の自動送信を行わず、既存の `perform_connect` を使うテストが非回帰であること。
- `accept` の doc に、ACK 未処理時の初期値 0 と回復動作が明記されていること。
- 追加した API の rustdoc に、`WtSession` の同名 API と同じ契約（非増加・存在しないストリーム・状態不正のエラー）が明記されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
