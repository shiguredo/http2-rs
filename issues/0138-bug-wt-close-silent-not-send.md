# 送信ウィンドウ枯渇時に close() が Ok を返しても WT_CLOSE_SESSION / END_STREAM が送信されない問題を修正する

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-close-silent-not-send
- Polished: 2026-09-09

## 目的

`crates/tokio-http2/src/webtransport.rs` の `WtServerSession::close()` / `WtSessionHandle::close()` が成功 (Ok) を返したにも関わらず、ピアへ WT_CLOSE_SESSION capsule や END_STREAM が実際には送信されないケースがあり、呼び出し側が「セッションは正常に終了した」と誤認する問題を修正する。

## 現状

`DriverState::handle_cmd` の Close コマンド処理は、`wt_session.close()` 成功後に `flush_wt_output()` と `conn.send_data(connect_stream_id, vec![], true)` を実行し、成功すれば ack に Ok を載せて返す。

ここで、CONNECT ストリームの送信ウィンドウが枯渇している場合 (クライアントが WINDOW_UPDATE を送らない等)、sans-io 層の `Connection::send_data` は以下の挙動となる:

- `src/connection.rs` の `queue_data` は送信バッファに余裕があればデータを積むだけでエラーにしない
- `flush_stream_data` は送信バッファが非空かつ `available == 0` だと `return Ok(())` して送信せず、バッファにデータを残したまま成功を返す (空 DATA + END_STREAM は RFC 9113 Section 6.9.1 によりウィンドウ 0 でも送信できるが、バッファが非空のため早期 return で到達しない)

このため close() は ack に Ok を載せて戻るが、WT_CLOSE_SESSION capsule は `send_buffer` に、END_STREAM は `pending_end_stream` フラグに残ったまま、driver 終了によるコネクション drop で破棄される。ピア側は接続断でセッションが異常終了したように見え、呼び出し側は「正常に close された」と誤認する。

送信バッファが完全に満杯の場合は `send buffer full` エラーになり ack 経由でエラーが伝わるが、バッファに余裕がある場合 (ウィンドウ枯渇による silent buffering) は Ok のまま戻るため、エラーにはならない。

## 設計方針

- sans-io 層の `Connection::send_data` が「送信できずにバッファへ積んだだけ」でも Ok を返す挙動は、RFC 9113 Section 6.9 の保留モデルとして正しく、0137 の完了条件 (ウィンドウ枯渇時はエラーにせず滞留させ、WINDOW_UPDATE 後に送信する) および既存テストが前提としている。したがってこの案は採らず、`send_data` の Ok 仕様は変更しない
- close() 側で「実際に送信されたか」を判定する。sans-io `Connection` に、指定ストリームの送信待ちデータ (`send_buffer`) または保留中の END_STREAM (`pending_end_stream`) が残っているかを返す公開メソッドを追加し、tokio-http2 の `Connection` / `ServerConnection` から委譲する。CODEBASE.md の公開 API 規約に従いテストを追加する
- `DriverState::handle_cmd` の Close 処理は、`flush_wt_output()` と `send_data(connect_stream_id, vec![], true)` の後に上記メソッドで CONNECT ストリームの送信待ちを検査し、残っていれば ack に Err を載せる。driver は Close で終了するため capsule はピアへ届かないが、呼び出し側は「送信されていない」ことを認識できる
- driver を終了させず WINDOW_UPDATE を待って送信する案は、ピアが WINDOW_UPDATE を送らない場合に close() がハングするため採らない
- 0134 は `send_response` / `send_trailers` に閉じ、`send_data` の Ok 返却仕様は本 issue に委ねると明記している。本 issue は `send_data` の仕様を変えず、0137 の保留モデルを維持する

## 完了条件

- 送信ウィンドウ枯渇時に close() を呼ぶと Err が返ること (E2E テストで検証)
- 正常時 (ウィンドウに余裕がある場合) は close() が Ok を返し、WT_CLOSE_SESSION と END_STREAM がピアへ届くこと
- 送信待ちデータの有無を返す新規公開 API のテストが追加されていること
- `cargo test --all` が通過すること
