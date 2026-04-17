# WebTransport の動的フロー制御を実装する

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

draft-ietf-webtrans-http2-14 の動的フロー制御カプセルを自動で発行するロジックを `tokio-http2` の WebTransport 層に組み込む。
対象は `WT_MAX_DATA` / `WT_MAX_STREAM_DATA` / `WT_MAX_STREAMS` (bidi/uni) の送信と、`WT_DATA_BLOCKED` / `WT_STREAM_DATA_BLOCKED` / `WT_STREAMS_BLOCKED` の送信。

## 背景

`WtFlowControl` は既に `src/webtransport/flow_control.rs` に実装されており、`consume_recv` / `consume_send` / `update_max_streams` / `update_send_max` は呼び出せる。
しかし、閾値を超えた際に自動で `WT_MAX_*` capsule を emit する高レベルポリシーは未実装。

## 根拠

- draft-ietf-webtrans-http2-14 Section 6.5〜6.10: これらの capsule が無いと、長時間稼働でセッションが固まる (スタベーション)
- 相互運用性: Chrome / Safari などのクライアントは WT_MAX_STREAMS を期待している

## 対応内容

### ポリシー

- 受信側:
  - `recv_max - recv_offset < recv_max / 2` で `WT_MAX_DATA` を送出し、`recv_max` を 2 倍に増やす (初期値は WtConfig から)
  - ストリーム単位も同様に `WT_MAX_STREAM_DATA`
  - ストリームが閉じた (send/recv ともに terminal) 数が閾値を超えたら `WT_MAX_STREAMS` を累積発行

- 送信側:
  - `send_available == 0` かつ送りたいデータがあるときに `WT_DATA_BLOCKED` / `WT_STREAM_DATA_BLOCKED` を 1 回だけ送出

### 実装場所

- `crates/tokio-http2/src/webtransport.rs` の `WtServerSession` 内ポーリング
- または glue (0004) のイベントループで tick ごとに評価

### 設定

- `WtConfig` にフロー制御自動発行の ON/OFF オプションを追加 (デフォルト ON)
- 閾値 (`max_data_threshold_ratio`, `max_streams_threshold`) を `WtConfig` に追加

## 完了条件

- `WT_MAX_*` capsule の送信頻度が適正 (単純エコーで 1 回以上送出されるのを統合テストで確認)
- `send_datagram`, `send_stream_data` が `flow control` エラーを返さない通常利用ケースが成立
- `cargo test --workspace` が通る

## 依存

- 0002, 0003, 0004, 0005

## 解決方法

- `src/webtransport/mod.rs` に以下の公開 API を追加
  - `WtSession::send_max_data` / `send_max_stream_data` / `send_max_streams` (capsule 直接送出)
  - `WtSession::grow_recv_window` / `grow_stream_recv_window` / `grow_max_streams` (高レベル)
  - `WtSession::flow_control` / `flow_control_mut` / `stream` / `config`
- `src/webtransport/flow_control.rs` に getter を追加 (`send_max`, `recv_max`, `max_streams_*_{local,remote}`)
- `src/webtransport/stream.rs` に getter を追加 (`send_max`, `recv_max`)
- `crates/tokio-http2/src/webtransport.rs` の `DriverState` に自動発行ロジックを追加
  - `maybe_grow_session_window`: セッション recv ウィンドウを `initial_max_data` 単位で拡張
  - `maybe_grow_stream_window`: ストリーム recv ウィンドウを `initial_max_stream_data_*` 単位で拡張
  - `maybe_grow_max_streams`: ピアが閉じたストリーム数が閾値を超えたら `WT_MAX_STREAMS` を送出
  - `account_peer_stream_closed`: クライアント起点ストリームのクローズをカウント
- `handle_event` / `dispatch_wt_event` から各 `maybe_grow_*` を呼び出し、データパスに組み込み
- 送信側 (`WT_*_BLOCKED`) 自動発行は将来の課題として残し、API のみ `WtSession` 側で利用可能な状態 (既存 `Capsule::WtDataBlocked` 等は encode 可能)
