# WtSession が Closed 状態でも受信 capsule を処理して新規ストリームを生成する

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-closed-recv-capsule
- Polished: 2026-08-24

## 目的

`WtSession` (`src/webtransport.rs`) が `WtSessionState::Closed` に遷移した後も、受信 capsule を処理し続け、WT_STREAM を受信すると新規ストリームを生成して `StreamOpened` イベントを送出する問題を修正する。draft-ietf-webtrans-http2-15 Section 6.12 の終端信号との整合を図る。

## 現状

`WtSession::handle_capsule` / `WtSession::handle_stream_data` の受信経路にはセッション状態のガードがない。`Capsule::WtCloseSession` 受信で `state = WtSessionState::Closed` になった後も、後続の capsule を処理し続ける:

- `handle_capsule` の `Capsule::WtStream` 分岐 → `handle_stream_data` → 新規ストリーム生成 (`WtStream::new`) + `StreamOpened` イベント
- `Capsule::Datagram` → `DatagramReceived` イベント
- その他の capsule も処理・イベント送出が継続

`WtSessionState::Closed` のガードは送信 API (`open_stream` / `send_stream_data` / `send_datagram` / `close`) にのみ存在し、受信側は非対称である。送信側では `prop_session_closed_is_absorbing` 等で「Closed は吸収状態」と検証済みだが、受信側の吸収性は未検証である。

## 設計方針

- `handle_capsule` / `handle_stream_data` の冒頭で `WtSessionState::Closed` を判定し、Closed 後の capsule は無視する (吸収状態として扱う)
- ピアからの WT_CLOSE_SESSION 後はイベント送出も停止する
- Closed 後の受信 capsule が無視されるテストを追加する

## 完了条件

- Closed 状態での WT_STREAM 受信で新規ストリームが生成されないこと
- Closed 状態での capsule 受信でイベントが送出されないこと
- テストが追加され、`cargo test --all` が通過すること
