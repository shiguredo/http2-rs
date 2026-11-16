# tokio WebTransport のコマンド API がキャンセル安全でない

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-command-cancellation
- Polished: 2026-11-17

## 目的

`WtServerSession` / `WtSessionHandle` / `WtBidiStream` / `WtUniRecvStream` /
`WtUniSendStream` のコマンド API は「`cmd_tx.send(...)` してから `oneshot` の
ack を await」する。`tokio::select!` や timeout で future を落とすと、driver は
コマンドを処理するが ack の受信者がいないため結果は捨てられる。ストリーム状態や
`DriverState::stream_channels` が残り、再試行でデータが二重送信される経路を修正する。
mpsc の受信 API (`accept_bidi` / `accept_uni` / `recv_datagram` / `recv`) は
キャンセル安全であり、動作の修正は対象外とする (rustdoc にはキャンセル安全で
あることを明記する)。

## 現状

`crates/tokio-http2/src/webtransport.rs` の次のコマンド API は `oneshot` で結果を待つ。

- `WtServerSession` / `WtSessionHandle`: `open_bidi` / `open_uni` / `send_datagram` / `export_keying_material` / `close` / `drain`
- `WtBidiStream` / `WtUniSendStream`: `send` / `reset` (`WtBidiStream` は `stop_sending` も)
- `WtUniRecvStream`: `stop_sending`

`DriverState::handle_cmd` はコマンドを処理し、`send_cmd_result` と `handle_cmd` 内の `ack.send` の戻り値を無視する (`let _ = ack.send(...)`)。ack の受信者が drop 済みでもコマンドは適用される。

`open_bidi` がキャンセルされると `DriverState::stream_channels` に送信者だけが残り、
返却予定だった `WtBidiStream` は失われる。`open_uni` はストリーム ID と累積
ストリーム数 (WT_MAX_STREAMS) の枠を消費したまま、`WtUniSendStream` と
`WtSession` のストリーム状態が残る。`send` は送信後に成功を知らず、再試行すると
同じデータを二重送信し得る。

`WtSession::open_bidi_stream` / `open_uni_stream` はストリームを開くだけで
WT 出力を生成しない。WT_MAX_STREAMS は累積値であり、閉じたストリームも含む
(draft-ietf-webtrans-http2-16 Section 6.7)。したがって、開設後に reset しても
消費したストリーム枠は戻らない。キャンセル時は開設前に ack の消失を確認し、
ストリームを開かないことが第一である。

## 設計方針

- 各コマンド API に `# Cancel safety` を明記し、キャンセル時の状態 (適用済み / 未適用 / 不定) を定義する。少なくとも次を区別する。
  - 状態を変えない、または状態変更前に ack の消失を検出してスキップできる操作: `export_keying_material`、mpsc の受信 API (`accept_bidi` / `accept_uni` / `recv_datagram` / `recv`)。
  - キャンセル時に適用済みか未適用か不定になる操作: `send`、`send_datagram`、`reset`、`stop_sending`、`drain`、`close`。
  - ストリームを生成する操作: `open_bidi`、`open_uni`。
- driver は `ack.is_closed()` をコマンド適用前に確認する。受信者がいなければ、
  `open_bidi` / `open_uni` はストリームを開かずにコマンドを破棄し、`send` /
  `send_datagram` / `reset` / `stop_sending` / `close` / `drain` は状態を変更せずに
  破棄する。`export_keying_material` は計算を省略してよい。
- 開設後に ack の消失が判明した場合 (適用前チェックと `ack.send` の間の競合) は、
  `ack.send` の `Err` 戻り値から返却できなかった `WtBidiStream` /
  `WtUniSendStream` を取り出し、そのストリームを reset / 削除する。`open_bidi` では
  `DriverState::stream_channels` の該当エントリも削除する。reset は 0179 の
  ピア既知追跡と空 WT_STREAM 送信を前提とする。この競合では累積ストリーム数は
  消費されたままであることを doc に明記する。
- ack の送信後に呼び出し側が future を落とした場合に備え、driver がストリーム
  ハンドルの drop を検出できるようにする。`open_bidi` は
  `DriverState::stream_channels` の送信者の `is_closed()` で検出できる。`open_uni`
  は `WtUniSendStream` にキャンセル検出用のチャネルを持たせ、driver が sender の
  `is_closed()` で検出する。検出したストリームは reset / 削除する。
- `send` の `Ok` / `Err` の意味は既存の契約に従う。キャンセル時は結果を観測できないため、送信済みか未送信か不定である。呼び出し側は同じデータを再送してはならない (または再送を許容する) と rustdoc に明記する。再送を安全にしたい場合はリクエスト ID と冪等性を持たせる。
- `close` は `WtServerSession::close` が self を消費するため、キャンセル時は
  セッションが drop され driver が abort される。WT_CLOSE_SESSION / END_STREAM が
  送信済みかは不定である。`WtSessionHandle::close` は handle が残るため再試行できるが、
  driver が既に終了していれば `Error::ConnectionClosed` になる。
- `drain` はキャンセル時に Draining へ遷移済みであり得る。再試行は `session_state_error` になり得ることを doc に明記する。
- `reset` / `stop_sending` はキャンセル時に適用済みであり得る。再試行は `stream_state_error` になり得ることを doc に明記する。
- `send_datagram` はキャンセル時に送信済みか不定である。DATAGRAM は再送を許容するか、再送しないことを doc に明記する。
- テストは current_thread ランタイムで、future を 1 回 poll してから drop することで、driver がコマンドを処理する前に ack の受信者が drop された状態を再現する (`std::task::Waker::noop()` と `Future::poll` を使う)。
  - `open_bidi` / `open_uni`: ピアが広告する `wt_initial_max_streams_bidi` / `wt_initial_max_streams_uni` を 1 にし、キャンセル後に再度 open して成功することを確認する。開設前チェックがなければ 2 回目がストリーム数上限で失敗する。
  - `send`: キャンセルした送信の後に別のデータを送り、ピアがキャンセルしたデータを受信しないことを確認する。
  - `close`: キャンセル後に driver が終了し、ハングしないことを確認する。
- 0180 で追加する `send_max_data` / `send_max_streams` / `send_max_stream_data` も同じキャンセル契約に含める。

## 依存

- 0178 / 0179: 開設後の競合で `reset_stream` を使う場合、ピア既知追跡と空 WT_STREAM 送信が必要。0179 を前提とする。0179 が未実装の間に実装する場合は、開設前チェックで競合を避け、競合時の reset は 0179 の実装後に行う。
- 0180: 追加されるコマンド API も本 issue の対象。
- 0183: `send_cmd_result` と送信キューの変更と整合させる。

## 完了条件

- キャンセルされた `open_bidi` / `open_uni` で、driver 内に未使用のストリーム状態と関連するチャネルが残らないこと。開設前の ack 消失チェックにより、通常のキャンセルはストリーム枠を消費しないこと。競合で開設後に reset した場合は、累積ストリーム数が消費されることを doc に明記すること。
- キャンセルされた `send` の再試行で二重送信が起きない契約 (再試行してはならない、またはリクエスト ID で冪等) が rustdoc に明記されていること。
- `WtServerSession` / `WtSessionHandle` / `WtBidiStream` / `WtUniRecvStream` / `WtUniSendStream` の各公開コマンドメソッドの rustdoc に、キャンセル時の状態 (適用済み / 未適用 / 不定) が明記されていること。mpsc の受信 API はキャンセル安全であることを明記すること。
- キャンセル経路のテストが追加されていること。少なくとも `open_bidi` / `open_uni` / `send` / `close` のキャンセルを再現し、driver がハングせず、ストリーム状態が残らないことを検証すること。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
