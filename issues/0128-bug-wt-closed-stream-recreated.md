# クローズ済みストリームへの WT_STREAM が新規ストリームとして再作成される

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-closed-stream-recreated
- Polished: {YYYY-MM-DD}

## 目的

WebTransport セッションにおいて、FIN やリセットでクローズ済みのストリーム ID 宛に後続の WT_STREAM capsule を受信した際、新規ストリームとして再作成される問題を修正する。draft-ietf-webtrans-http2-15 Section 6.4 の MUST 違反であり、アプリに既に終わったストリームの `StreamOpened` が再発行される。

## 現状

`src/webtransport.rs` の `WtSession::poll_event` は `WtEvent::StreamData { fin: true }` を pop した時点で `remove_if_closed` によりストリームを `streams` から削除する。その後、同じストリーム ID への WT_STREAM capsule を受信すると、`WtSession::handle_stream_data` の `is_new_stream = !self.streams.contains_key(&stream_id)` が true になり、クローズ済みストリームが新規ストリームとして再作成される (`WtStream::new` → `StreamOpened` イベント送出)。

draft-ietf-webtrans-http2-15 Section 6.4 (L977-L982) は「A WT_STREAM capsule MUST NOT be sent after a stream is closed or reset. ... A stream error of type WT_STREAM_STATE_ERROR MUST be sent if a WT_STREAM capsule is received for a stream that is not in a valid state」と定めており、WT_STREAM_STATE_ERROR のストリームエラーを要求する。

対照的に、`WtSession::handle_capsule` の `Capsule::WtResetStream` 分岐は未知ストリームを正しくエラーにしており、WT_STREAM 側だけが非対称に再作成を許す。

tokio ドライバ (`crates/tokio-http2/src/webtransport.rs` の `DriverState::handle_event`) はイベントを即時ドレインするため、サーバーで現実に到達する経路である。

## 設計方針

- クローズ済みストリーム ID を記録する仕組みを追加し (`remove_if_closed` で削除する際に ID を保持)、`handle_stream_data` でクローズ済み ID への WT_STREAM を `WtError::stream_state_error` で拒否する
- `WT_RESET_STREAM` 側と同様のエラー処理に揃える
- クローズ済み ID の記録は無制限に肥大化しないよう上限を持つ (HTTP/2 側の `BoundedClosedStreams` と同様の設計を参考にする)

## 完了条件

- FIN でクローズ済みのストリーム ID への WT_STREAM 受信が `stream_state_error` を返すこと
- リセットでクローズ済みのストリーム ID への WT_STREAM 受信が `stream_state_error` を返すこと
- 新規ストリーム ID への WT_STREAM は従来どおり `StreamOpened` を生成すること
- テストが追加され、`cargo test --all` が通過すること
