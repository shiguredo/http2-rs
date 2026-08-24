# 送信ウィンドウ枯渇時に close() が Ok を返しても WT_CLOSE_SESSION / END_STREAM が送信されない問題を修正する

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-close-silent-not-send
- Polished: {YYYY-MM-DD}

## 目的

`crates/tokio-http2/src/webtransport.rs` の `WtServerSession::close()` / `WtSessionHandle::close()` が成功 (Ok) を返したにも関わらず、ピアへ WT_CLOSE_SESSION capsule や END_STREAM が実際には送信されないケースがあり、呼び出し側が「セッションは正常に終了した」と誤認する問題を修正する。

## 現状

`DriverState::handle_cmd` の Close コマンド処理は、`wt_session.close()` 成功後に `flush_wt_output()` と `conn.send_data(connect_stream_id, vec![], true)` を実行し、成功すれば ack に Ok を載せて返す。

ここで、CONNECT ストリームの送信ウィンドウが枯渇している場合 (クライアントが WINDOW_UPDATE を送らない等)、sans-io 層の `Connection::send_data` は以下の挙動となる:

- `src/connection.rs` の `queue_data` は送信バッファに余裕があればデータを積むだけでエラーにしない
- `flush_stream_data` はウィンドウが 0 (`available == 0`) だと `return Ok(())` して送信せず、バッファにデータを残したまま成功を返す

このため close() は ack に Ok を載せて戻るが、WT_CLOSE_SESSION capsule と END_STREAM は送信バッファに積まれたまま、driver 終了によるコネクション drop で破棄される。ピア側は接続断でセッションが異常終了したように見え、呼び出し側は「正常に close された」と誤認する。

送信バッファが完全に満杯の場合は `send buffer full` エラーになり ack 経由でエラーが伝わるが、バッファに余裕がある場合 (ウィンドウ枯渇による silent buffering) は Ok のまま戻るため、エラーにはならない。

## 設計方針

- 本質的な原因は sans-io 層の `Connection::send_data` が「送信できずにバッファへ積んだだけ」の状態でも Ok を返すことにある。`src/connection.rs` の `send_data` / `flush_stream_data` の挙動を見直し、送信できなかったデータが残る場合に Ok を返さない (エラーにする、または送信待ち状態を明示する) ことが本筋
- もしくは、close() が Ok を返す条件を「END_STREAM が実際に送信できた場合」に限定する
- sans-io 層の変更は影響範囲が広い (全 send 経路に波及する) ため、実装方法の決定には設計判断が必要。方針が割れる場合は実装前に確認する

## 完了条件

- 送信ウィンドウ枯渇時に close() を呼んだ場合、呼び出し側が「実際には送信されていない」ことを認識できる (エラーが返る、またはドキュメント・型で明示される)
- 正常時 (ウィンドウに余裕がある場合) は従来通り close() が成功し、WT_CLOSE_SESSION と END_STREAM がピアへ届くこと
- `cargo test -p tokio-http2` が全件通過すること
