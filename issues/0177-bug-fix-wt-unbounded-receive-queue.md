# WebTransport 受信データの無制限キューイングと自動 credit を解消する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-unbounded-receive-queue
- Polished: 2026-11-17

## 目的

`tokio-http2` の WebTransport driver は bidi / uni / DATAGRAM / 各ストリームの受信に `mpsc::unbounded_channel()` を使い、アプリが受信しない場合でもデータを無制限に蓄積する。さらに受信量だけを基準に `WT_MAX_DATA` / `WT_MAX_STREAM_DATA` と HTTP/2 `WINDOW_UPDATE` を自動送信するため、遅いアプリでもピアは送り続けられる。リモートからのメモリ DoS を防ぐため、受信バッファを bounded にし、アプリが消費するまで credit を返さない設計に変更する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState` は `bidi_tx` / `uni_tx` / `datagram_tx` と各ストリームの `stream_channels` を unbounded channel で生成する。`dispatch_wt_event` は `StreamData` をそのまま channel へ送り、`maybe_grow_session_window` / `maybe_grow_stream_window` はアプリの消費ではなく `WtSession` の受信カウント (`recv_available`) だけを見てウィンドウを拡張する。`handle_event` は CONNECT ストリームの `Event::DataReceived` ごとに `send_window_update` を呼び、受信した DATA 長を即座に返す。

DATAGRAM は WebTransport のフロー制御対象外であり、`datagram_tx` も unbounded である。draft-ietf-webtrans-http2-16 Section 4.4 は消費に応じた credit 返却を SHOULD、Section 6.11 は DATAGRAM の破棄を許容している。RFC 9297 Section 3.2 も capsule 全量の蓄積を避けるよう求めている。

受信チャネルの型は `WtServerSession::into_parts` が返す `WtSessionParts` の公開フィールド (`bidi_rx` / `uni_rx` / `datagram_rx`) にも現れる。既存テストの `test_wt_max_data_auto_grows_session_window` と `test_wt_local_bidi_window_grows_with_asymmetric_limits` は、アプリが受信データを消費していない状態でも driver がウィンドウを拡張することを前提にしている。

## 設計方針

- 受信チャネル (`bidi_tx` / `uni_tx` / `datagram_tx` / `stream_channels`) を `mpsc::channel` の bounded にする。`cmd_tx` は unbounded のままとする。容量は固定値（例: 1 メッセージ）とし、`WtSessionParts` の公開フィールド型が `mpsc::Receiver` に変わることは許容する。
- driver は bounded channel への送信でコマンド処理を止めない。`try_send` と保留を使い、保留がある間も他のストリームのイベントとコマンドを処理し続ける。`mpsc::Sender::reserve()` で空きを待つ場合も、コマンド受信と `conn.next_event()` を同時に待つ。ストリームデータの channel が満杯のときは、そのストリームのパケットを保留し、他のストリームのイベントは処理し続ける。ストリームデータの保留の合計は WT ウィンドウで有界、ストリーム開設の保留はストリーム数上限で有界なので、`conn.next_event()` を一律に止めない。channel の空きは `reserve()` またはアプリの `accept_bidi` / `accept_uni` / `recv` が送る消費通知コマンドで再開する。これにより、`WtBidiStream::send` / `WtUniSendStream::send` の ack 待ちと受信キューの満杯が相互待ちにならない。
- アプリが WT_STREAM のデータを消費したら、`WT_MAX_DATA` / `WT_MAX_STREAM_DATA` を Stream Data 長の分だけ増やす。消費量はセッションレベルは全体、ストリームレベルはストリームごとに累積し、その累積が初期ウィンドウの半分以上になったら返す（現行の閾値方針を維持し、基準を受信量から消費量に変える）。増分が 0 のときは capsule を送らない。
- HTTP/2 の WINDOW_UPDATE は、WT_STREAM capsule についてはアプリが消費した capsule の type / length ヘッダーを含む HTTP/2 DATA バイト数で返す。DATAGRAM / WT_MAX_* / WT_CLOSE_SESSION などの非 WT_STREAM capsule は、driver が処理した時点で返す。WT_MAX_DATA は Stream Data 長、HTTP/2 WINDOW_UPDATE は capsule のエンコード後サイズであり、両者の単位が異なることに注意する。driver が capsule のエンコード後サイズを得るために、`WtEvent::StreamData` にエンコード後サイズを追加する、または driver 側で capsule の varint ヘッダー長を計算してエンコード後サイズを求める。
- FIN 付きデータの消費では、ストリームレベルの `WT_MAX_STREAM_DATA` を送らない（0153 / 0157 の非回帰）。セッションレベルの `WT_MAX_DATA` と HTTP/2 connection WINDOW_UPDATE は返す。ストリームレベルの拡張は `WtStream::recv_state()` が `Recv` のときだけ行う。
- `stop_sending_sent_streams` の破棄データは、アプリ消費ではないが driver が消費したものとして、セッションレベルの `WT_MAX_DATA` と HTTP/2 WINDOW_UPDATE を返す。ストリームレベルの `WT_MAX_STREAM_DATA` は送らない。受信側が drop されたストリームの未消費データも同様に扱う。
- DATAGRAM は bounded channel と 1 件あたりの最大サイズ（例: 64 KiB）で上限を設ける。上限超過時は新規 DATAGRAM を破棄し、キュー済みの DATAGRAM は保持する。driver は DATAGRAM の送信でブロックしない。破棄数は driver 内部で数え、`WtServerSession` / `WtSessionParts` から取得できる API を追加するか、追加しない場合は破棄数を doc の既知の制限として記載する。いずれの場合も上限値と破棄方針を rustdoc に明記する。公開 API を追加する場合は `CODEBASE.md` に従いテストを追加する。
- 遅い消費者を模した統合テストで、ピアの WT 送信可能量が初期ウィンドウで止まり、アプリが消費すると再開することを検証する。driver のメモリ使用量を直接測るのではなく、ピアの送信可能量と channel 容量の組み合わせで、受信キューに保持されるデータ量が上限以下であることを観測する。
- 既存テスト `test_wt_max_data_auto_grows_session_window` / `test_wt_local_bidi_window_grows_with_asymmetric_limits` は、アプリが受信データを消費する形に更新する。`test_wt_stream_window_grows_with_initial_one` / `test_wt_stream_window_not_grown_after_fin` は非回帰を確認する。

## 完了条件

- アプリが受信しない状態でピアが WT データを送り続けても、ピアの WT 送信可能量がサーバーが広告した初期ウィンドウで止まり、driver の受信キューに保持されるデータ量がその上限を超えないこと。
- アプリが受信データを消費すると、`WT_MAX_DATA` と、ストリームが `Recv` の場合は `WT_MAX_STREAM_DATA`、および HTTP/2 WINDOW_UPDATE が返り、ピアが送信を再開できること。
- FIN 付きデータの消費でストリームレベルの `WT_MAX_STREAM_DATA` を送らないこと、STOP_SENDING 後の破棄データでセッションレベルの credit を返すことが、テストで固定されていること。
- DATAGRAM の受信バッファ上限と破棄方針が実装され、rustdoc に明記されていること。上限超過時にセッションが継続すること。
- 遅い消費者と DATAGRAM 破棄の回帰テストが追加されていること。
- 既存の自動拡張テストが消費ベースの挙動に更新され、`test_wt_stream_window_grows_with_initial_one` / `test_wt_stream_window_not_grown_after_fin` が非回帰であること。
- `WtSessionParts` の受信チャネル型の変更が rustdoc に反映されていること。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
