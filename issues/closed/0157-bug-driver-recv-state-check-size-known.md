# ドライバのウィンドウ拡張判定が SizeKnown で受信状態検証と食い違う

- Created: 2026-09-12
- Completed: 2026-09-12
- Branch: feature/fix-driver-recv-state-check
- Polished: {YYYY-MM-DD}

## 目的

tokio ドライバが受信ウィンドウを拡張してよいかを判定する基準を、sans-io 層の検証と同じ基準にそろえる。現在の判定は `WtStream::can_recv()` を使っており、`WtSession::grow_stream_recv_window` が拒否する `SizeKnown` を通してしまう。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` は `WtStream::can_recv()` が偽なら拡張せずに戻る。`WtStream::can_recv()` は `Recv` と `SizeKnown` の両方で真になる (`src/webtransport/stream.rs` の `RecvState::can_recv`)。

一方 `WtSession::grow_stream_recv_window` は受信状態が `Recv` でなければ `stream_state_error` を返す (`SizeKnown` は拒否される)。したがって受信状態が `SizeKnown` のストリームで `maybe_grow_stream_window` が拡張を試みると、エラーが `DriverState::abort_session_with_wt_error` に伝播し、CONNECT ストリームへ RST_STREAM を送ってセッションを終了させる。

`SizeKnown` は現行実装では到達しない (`WtStream::recv_data` は FIN 付き受信で `SizeKnown` を経由せず `DataRecvd` へ直接遷移する) ため現時点で実害はないが、到達可能になった時点で正常な通信がセッション終了になる。ドライバのコメントも `SizeKnown` が到達しないことを前提としており、判定の根拠がコードの契約と一致していない。

## 設計方針

- `maybe_grow_stream_window` の判定を `WtStream::can_recv()` から `stream.recv_state() != RecvState::Recv` に変更し、受信状態が `Recv` のときだけ拡張する。`RecvState` は `shiguredo_http2::webtransport` から取得できる
- しきい値の計算 (`initial.div_ceil(2)`)、開始主体ごとの `initial` の選択、`grow_stream_recv_window` の呼び出し位置は変更しない
- `SizeKnown` が到達しないことを前提とするコメントを、判定の根拠 (sans-io 層の検証と同じ基準にそろえる) を述べる内容に書き換える
- FIN を受信したストリームで拡張しない挙動 (0153 の非回帰) を維持する

## 完了条件

- `DriverState::maybe_grow_stream_window` の判定が `WtStream::recv_state()` と `RecvState::Recv` の比較になり、`WtStream::can_recv()` を使っていないこと (`WtSession::check_max_stream_data_recv_state` と同じ基準)。`SizeKnown` は現行実装で到達しないため、この項目はコードレビューで確認する
- FIN を受信したストリームのウィンドウが拡張されないことが非回帰であること (`test_wt_stream_window_not_grown_after_fin`)
- `Recv` 状態のストリームのウィンドウが従来どおり拡張されること (`test_wt_local_bidi_window_grows_with_asymmetric_limits` / `test_wt_stream_window_grows_with_initial_one`)
- `cargo test --all` が通過すること (判定基準の変更のみで現行の挙動は変わらないため、新しいテストの追加は求めない)

## 解決方法

本 issue では実装せず、`issues/0158-remove-unreachable-stream-states.md` で解消する。

- 本 issue の食い違いは、`RecvState::SizeKnown` が到達しないのに `WtStream::can_recv()` がそれを許容していることに由来する。到達しない variant を削除すれば `can_recv()` は `Recv` のみ真になり、`WtSession::check_max_stream_data_recv_state` の `Recv` 限定検証と意味が一致する。ドライバの判定 (`DriverState::maybe_grow_stream_window` の `can_recv()`) は変更不要になる
- 到達不能な variant を残したままドライバ側だけを `recv_state()` 比較に変える案も検討したが、`CODEBASE.md` の「未使用・テスト未使用の公開 API はテストを追加して動作を保証するか削除して解消する」に照らすと、`WtStream` の状態を設定する公開 API が無くテストで保証できないため、削除で解消するのが本筋と判断した
- 0158 の完了時点で本 issue が扱っていた経路は消滅するため、本 issue は実装せず closed にする
