# tokio-http2 のフロー制御 API 欠落と接続レベルウィンドウの初期化・広告の不備を修正する

- Priority: High
- Created: 2026-05-24
- Model: Opus 4.7
- Branch: feature/fix-flow-control-window-update

## 目的

tokio-http2 ラッパー層と Sans I/O 層の接続レベルフロー制御に以下の問題があり、65535 bytes を超えるボディの送受信時にフリーズする。外部ユーザーからのフィードバックで報告された。

1. `Client` / `ServerConnection` に `send_window_update` のデリゲーションメソッドが公開されていない（Sans I/O 層と `tokio-http2::Connection` には既に実装済み）
2. 接続レベルの `FlowControl` が `send_window` をローカルの `connection_window_size` で初期化しており、ピアのデフォルト (65535) と乖離する
3. `initiate()` / `send_settings()` で `connection_window_size` がデフォルトより大きい場合に接続レベルの WINDOW_UPDATE を送信しない
4. examples/http2_client と examples/http2_server が DATA 受信時に WINDOW_UPDATE を送信しない
5. WebTransport ドライバー (`crates/tokio-http2/src/webtransport.rs`) が `DataReceived` 処理時に HTTP/2 レベルの接続・ストリーム WINDOW_UPDATE を送信しない

## 優先度根拠

外部ユーザーからのフィードバックで報告された実用上のバグ。HTTP レスポンスが 65535 bytes を超えることは一般的であり、基本的なフロー制御が機能しない。

## 現状

### Client / ServerConnection に `send_window_update` が公開されていない

`tokio-http2::Connection` (`crates/tokio-http2/src/connection.rs`) には `send_window_update` メソッドがあるが、`Client` (`crates/tokio-http2/src/client.rs`) と `ServerConnection` (`crates/tokio-http2/src/server.rs`) にはデリゲーションメソッドが公開されていない。

### 接続レベル FlowControl の初期化が不正

`Connection::new()` (`src/connection/mod.rs`) で `FlowControl::new(limits.connection_window_size)` を呼んでいるが、`FlowControl::new()` は `send_window` と `recv_window` の両方を同じ値で初期化する。接続レベルの `send_window` はピアの受信ウィンドウに対応し、接続確立時点ではデフォルト 65535 で初期化すべき。

### `connection_window_size` の値が広告されない

接続レベルのウィンドウをデフォルト (65535) より大きくする場合、接続確立直後に WINDOW_UPDATE フレームで差分を送信する必要がある。

Sans I/O 層には SETTINGS を送信する経路が 2 つある:
- `initiate()`: クライアントプリフェイス文字列 + SETTINGS を送信
- `send_settings()`: SETTINGS のみを送信（tokio-http2 ラッパーの `initiate()` がこちらを呼ぶ）

どちらの経路も WINDOW_UPDATE を送信していない。

### examples が WINDOW_UPDATE を送信しない

`examples/http2_client/src/main.rs` と `examples/http2_server/src/main.rs` の `DataReceived` イベントハンドラで WINDOW_UPDATE を一切送信していない。

### WebTransport ドライバーが HTTP/2 レベルの WINDOW_UPDATE を送信しない

`crates/tokio-http2/src/webtransport.rs` の `handle_event()` で `DataReceived` を処理する際、WebTransport レベルのフロー制御 (`maybe_grow_session_window()` / `maybe_grow_max_streams()`) は行っているが、HTTP/2 レベルの接続・ストリーム WINDOW_UPDATE を送信していない。WebTransport セッションは単一の CONNECT ストリーム上で大量のデータをやり取りするため、65535 bytes でフリーズする。

### 再現手順

1. `examples/http2_server` を起動する
2. `examples/http2_server/src/main.rs` のレスポンスボディを 65535 bytes 超に変更する（例: `"X".repeat(100_000)` を返す）
3. `examples/http2_client` でリクエストを送信する
4. 65535 bytes 受信した時点でサーバーの送信ウィンドウが枯渇しフリーズする

## 設計方針

### RFC 9113 根拠

- Section 6.5.2: `SETTINGS_INITIAL_WINDOW_SIZE` (0x04) はストリームレベルのフロー制御の初期ウィンドウサイズであり、接続レベルには適用されない
- Section 6.9: ストリーム識別子 0 の WINDOW_UPDATE は接続全体を対象とする
- Section 6.9.1: 受信者は DATA フレームの受信を接続フロー制御ウィンドウに常に計上しなければならない (MUST)。ストリームレベルと接続レベルで個別に WINDOW_UPDATE を送信する
- Section 6.9.2: 接続フロー制御ウィンドウは WINDOW_UPDATE フレームでのみ変更可能。SETTINGS フレームでは変更できない

### 対応方針

#### 1. `Client` と `ServerConnection` にデリゲーションメソッドを追加する

`crates/tokio-http2/src/client.rs` と `crates/tokio-http2/src/server.rs` に `send_window_update` メソッドを追加する。内部の `self.conn.send_window_update()` に委譲するだけの薄いラッパー。

#### 2. 接続レベル FlowControl の初期化を修正する

`src/connection/mod.rs` の `Connection::new()` で `FlowControl::new(limits.connection_window_size)` を `FlowControl::new(DEFAULT_INITIAL_WINDOW_SIZE)` に変更する。

これにより `send_window` と `recv_window` の両方が 65535 で初期化される。`connection_window_size` がデフォルトより大きい場合は、対応方針 3 の WINDOW_UPDATE 送信時に `send_window_update()` 内部の `add_recv_window()` で `recv_window` が `connection_window_size` まで増加する。

この方式により:
- `send_window` がピアのデフォルト (65535) と一致する
- `recv_window` の二重カウントが発生しない（`send_window_update()` の `add_recv_window()` で 1 回だけ加算される）
- issue 0014 (`FlowControl::with_separate_windows` のバグ) との依存がなくなる

#### 3. `initiate()` と `send_settings()` の両方に WINDOW_UPDATE 送信を追加する

`src/connection/mod.rs` の `initiate()` と `send_settings()` の末尾で、`connection_window_size > DEFAULT_INITIAL_WINDOW_SIZE` の場合に `self.send_window_update(StreamId::Connection, connection_window_size - DEFAULT_INITIAL_WINDOW_SIZE)` を呼ぶ。

`connection_window_size` の値を参照するため、`Connection` に `connection_window_size: u32` フィールドを追加し、`new()` で `limits.connection_window_size` を保存する。`Limits` 全体を保持すると `local_settings` と重複フィールドが生じるため、単独フィールドが適切。

WINDOW_UPDATE の二重送信を防ぐため、`Connection` に `connection_window_update_sent: bool` フラグを追加する。`initiate()` は `preface_sent` で二重呼び出しが防止されるが、`send_settings()` にはそのようなガードがないため、このフラグで初回のみ WINDOW_UPDATE を送信する。

**対応方針 2 と 3 は不可分**: 対応方針 2 だけ適用すると `send_window` が 65535 に下がり、ピアが `connection_window_size` 分のデータを送信できなくなる。対応方針 3 の WINDOW_UPDATE でピアの `send_window` を増加させる必要がある。部分適用は既存テスト (`test_custom_initial_window_size`) のリグレッションを引き起こす。

#### 4. `with_connection_window_size` に下限チェックを追加する

`src/limits.rs` の `with_connection_window_size()` に `assert!(size >= DEFAULT_INITIAL_WINDOW_SIZE)` を追加する。RFC 9113 では接続レベルのウィンドウを SETTINGS で縮小する手段がなく、負の WINDOW_UPDATE も存在しない。デフォルト未満の値を設定しても意図を実現できないため、構築時に拒否する。

#### 5. WebTransport ドライバーの修正

`crates/tokio-http2/src/webtransport.rs` の `handle_event()` で `DataReceived` を処理した後、HTTP/2 レベルの接続レベル (`StreamId::Connection`) とストリームレベル (`connect_stream_id`) の WINDOW_UPDATE を送信する。increment は受信した `data.len()` を使用する。WebTransport レベルのフロー制御 (`maybe_grow_session_window()` 等) の直後に追加する。

#### 6. examples の修正

`examples/http2_client/src/main.rs` と `examples/http2_server/src/main.rs` で DATA 受信時に接続レベル (`StreamId::Connection`) とストリームレベルの WINDOW_UPDATE を送信する。

increment は受信した `data.len()` を使用する。`end_stream == true` の場合、ストリームレベルの WINDOW_UPDATE は不要（直後にストリームが closed になるため）。接続レベルの WINDOW_UPDATE は `end_stream` に関わらず送信する（他のストリームの DATA 受信余地を維持するため）。

### 実装順序

対応方針には依存関係がある。以下の順序で実装する:

1. 対応方針 1 (デリゲーションメソッド追加) -- 対応方針 5, 6 の前提
2. 対応方針 2 + 3 (FlowControl 初期化修正 + WINDOW_UPDATE 送信) -- 不可分、同時に実装
3. 対応方針 4 (下限チェック追加) -- 独立
4. 対応方針 5 (WebTransport 修正) -- 対応方針 1 の完了が前提
5. 対応方針 6 (examples 修正) -- 対応方針 1 の完了が前提

### エッジケース

- `connection_window_size == DEFAULT_INITIAL_WINDOW_SIZE` (65535) の場合: `initiate()` / `send_settings()` で WINDOW_UPDATE を送信しないこと
- `connection_window_size == MAX_WINDOW_SIZE` (2^31-1) の場合: increment が正しく計算されること
- ストリームが closed になった後のストリームレベル WINDOW_UPDATE: 無視される (RFC 9113 Section 6.9)

### スコープ外

- パディング付き DATA フレームのフロー制御消費量と `DataReceived` イベントの `data.len()` の不一致: `Event::DataReceived` に `flow_control_size` フィールドを追加する設計変更を別 issue で検討する
- Sans I/O 層での自動 WINDOW_UPDATE 送信: RFC 9113 Section 5.2.1 項目 7 で「受信者がいつ WINDOW_UPDATE を送信するかは規定しない」とされており、自動送信の設計は別 issue で検討する
- `Client` に存在しない他の API (`reset_stream` 等) の非対称性: 別 issue で対応する
- 接続レベル `FlowControl` の `should_send_window_update()` / `window_update_increment()` の閾値が `initial_window_size` (65535) 基準になる問題: `connection_window_size` が大きい場合に閾値が低すぎる可能性がある。Sans I/O 層の直接利用者にのみ影響し、tokio-http2 ラッパー利用者には影響しない。別 issue で対応する

## 完了条件

- `Client` と `ServerConnection` から `send_window_update` を呼べる
- `Connection::new()` の接続レベル `send_window` が `DEFAULT_INITIAL_WINDOW_SIZE` (65535) で初期化される
- `connection_window_size` がデフォルトより大きい場合、`initiate()` と `send_settings()` の両方で WINDOW_UPDATE が送信される
- `connection_window_size == DEFAULT_INITIAL_WINDOW_SIZE` の場合、WINDOW_UPDATE が送信されない
- デフォルトウィンドウサイズ (65535) で 65535 bytes 超のレスポンスボディを送受信する統合テスト (`crates/tokio-http2/tests/client_server.rs`) が通る
- `connection_window_size` を `(DEFAULT_INITIAL_WINDOW_SIZE + 1)..=MAX_WINDOW_SIZE` の範囲で生成し、`initiate()` 後の出力に含まれる WINDOW_UPDATE の increment が `connection_window_size - DEFAULT_INITIAL_WINDOW_SIZE` であることを検証する PBT (`pbt/tests/prop_connection.rs`) が通る（`DEFAULT_INITIAL_WINDOW_SIZE` ちょうどの場合は increment = 0 となり `send_window_update` が拒否するため、WINDOW_UPDATE なしを完了条件 4 行上で別途検証する）
- `with_connection_window_size` が `DEFAULT_INITIAL_WINDOW_SIZE` 未満の値を拒否する
- WebTransport ドライバーが `DataReceived` 処理時に HTTP/2 レベルの WINDOW_UPDATE を送信する
- examples が 65535 bytes を超えるボディを正常に送受信できる
- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る

## CHANGES.md エントリ案

- [ADD] `Client` と `ServerConnection` に `send_window_update` メソッドを追加する
  - @voluntas
- [FIX] 接続レベルの `FlowControl` が `send_window` をローカルの `connection_window_size` で初期化するバグを修正する
  - @voluntas
- [FIX] `connection_window_size` をデフォルトより大きく設定した場合に接続確立時の WINDOW_UPDATE 送信が欠落するバグを修正する
  - @voluntas
- [FIX] WebTransport ドライバーが `DataReceived` 処理時に HTTP/2 レベルの接続・ストリーム WINDOW_UPDATE を送信しないバグを修正する
  - @voluntas

### misc

- [FIX] examples/http2_client と examples/http2_server で DATA 受信時に WINDOW_UPDATE を送信するよう修正する
  - @voluntas
