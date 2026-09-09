# WtSession が Closed 状態でも受信 capsule を処理して新規ストリームを生成する

- Created: 2026-08-24
- Completed: 2026-09-09
- Branch: feature/fix-wt-closed-recv-capsule
- Polished: 2026-09-09

## 目的

`WtSession` (`src/webtransport.rs`) が `WtSessionState::Closed` に遷移した後も、受信 capsule を処理し続け、WT_STREAM を受信すると新規ストリームを生成して `StreamOpened` イベントを送出する問題を修正する。draft-ietf-webtrans-http2-15 Section 6.12 の終端信号との整合を図る。

## 現状

`WtSession::handle_capsule` / `WtSession::handle_stream_data` の一般の受信経路 (WT_STREAM / Datagram) にはセッション状態のガードがない。`Capsule::WtCloseSession` 受信で `state = WtSessionState::Closed` になった後も、後続の capsule を処理し続ける:

- `handle_capsule` の `Capsule::WtStream` 分岐 → `handle_stream_data` → 新規ストリーム生成 (`WtStream::new`) + `StreamOpened` イベント
- `Capsule::Datagram` → `DatagramReceived` イベント
- その他の capsule も処理・イベント送出が継続

`WtSessionState::Closed` のガードは送信 API (`open_stream` / `send_stream_data` / `send_datagram` / `close`) に存在する一方、一般の受信経路 (`Capsule::WtStream` / `Capsule::Datagram` など) にはない。`Capsule::WtCloseSession` / `Capsule::WtDrainSession` の分岐だけは個別に状態を見て冪等化しているが、セッション全体の吸収性は保証していない。`prop_session_closed_is_absorbing` は `RecvCloseSession` / `RecvDrainSession` の状態吸収を検証済みだが、WT_STREAM / Datagram 受信時のイベント送出とストリーム生成は未検証である。

## 設計方針

- セッション状態 `Closed` のガードを `handle_capsule` の冒頭に一本化し、Closed 後の capsule は無視する (吸収状態として扱う)。`handle_stream_data` は `handle_capsule` の `Capsule::WtStream` 分岐からのみ呼ばれるため、両方にガードを置くと `handle_stream_data` 側が到達不能になる。`handle_capsule` 冒頭のガードにより `Capsule::WtCloseSession` 分岐の既存 `Closed` 判定は冗長になるが、`Capsule::WtDrainSession` 分岐の `Active` 判定は Draining の冪等性のために引き続き必要
- ピアからの WT_CLOSE_SESSION 後はイベント送出も停止する
- 「無視する」根拠: Section 6.12 は WT_CLOSE_SESSION 受信時に END_STREAM 応答でストリームを閉じることを MUST とする。ここで Section 6.4 の WT_STREAM_STATE_ERROR (ストリームエラー) を返すと `process` が `Err` になり、ドライバは RST_STREAM 経路に落ちて END_STREAM 応答に到達できない。したがってセッション単位の `Closed` は無視し、ストリーム単位のクローズ済み ID を `stream_state_error` にする 0128 とは適用条件を分ける
- 0128 は `remove_if_closed` で記録したクローズ済みストリーム ID への WT_STREAM を `handle_stream_data` で拒否する。本 issue の `handle_capsule` 冒頭のセッション `Closed` ガードの方が先に評価されるため、両者は衝突しない
- Closed 後の受信 capsule が無視されるテストを追加する

## 完了条件

- Closed 状態での WT_STREAM 受信で新規ストリームが生成されないこと (ピア開始ストリーム ID を使う。ローカル開始 ID は `handle_stream_data` の `is_peer_initiated` 検証で修正前でも拒否されるため回帰テストにならない。修正前は `StreamOpened` が生成されることを確認する)
- Closed 状態での capsule 受信でイベントが送出されないこと
- テストが追加され、`cargo test --all` が通過すること

## 解決方法

- `src/webtransport.rs` の `WtSession::handle_capsule` 冒頭に `WtSessionState::Closed` ガードを追加し、Closed 後の受信 capsule を無視するようにした (吸収状態)。`handle_stream_data` は `handle_capsule` からのみ呼ばれるため、ガードは `handle_capsule` に一本化した
- ガードにより到達不能になった `Capsule::WtCloseSession` 分岐の `Closed` 判定を削除した。`Capsule::WtDrainSession` 分岐の `Active` 判定は Draining の冪等性のために維持した
- `WtSession::process` の doc に、Closed 時は受信 capsule を無視して `Ok(())` を返すことを追記した
- `tests/test_webtransport/integration.rs` に、同一 DATA フレーム内で WT_CLOSE_SESSION → WT_STREAM (ピア開始 ID) → Datagram → 未知ストリームへの WT_RESET_STREAM を流し、新規ストリームが生成されず `SessionClosed` 以外のイベントが送出されず `process` がエラーにならないことを検証するテストを追加した
- `CHANGES.md` の `## develop` に `[FIX]` エントリを追加した
