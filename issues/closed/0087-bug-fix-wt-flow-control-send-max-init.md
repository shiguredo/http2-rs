# WtFlowControl / WtStream の send_max と recv_max を別々に初期化する

- Created: 2026-07-30
- Completed: 2026-07-30
- Branch: feature/fix-wt-flow-control-send-max-init
- Polished: 2026-07-30

## 目的

`WtFlowControl::new` と `WtStream::new` が `send_max`（ピアが許可した送信上限）と `recv_max`（ローカルが許可した受信上限）を同一値で初期化しているバグを修正する。同様に `max_streams_bidi/uni` の local/remote も同一値で初期化しているため、あわせて分離する。

## 現状

`src/webtransport/flow_control.rs` の `WtFlowControl::new` は以下の同一初期化を行っている:

- `send_max` と `recv_max` の両方を `initial_max_data`（ローカル設定値）で初期化
- `max_streams_bidi_local` と `max_streams_bidi_remote` の両方を `max_streams_bidi` で初期化
- `max_streams_uni_local` と `max_streams_uni_remote` の両方を `max_streams_uni` で初期化

`src/webtransport/stream.rs` の `WtStream::new` も `send_max` と `recv_max` を同一の `initial_max_data` で初期化している。

draft-ietf-webtrans-http2-15 Section 4.3.1 では、サーバーの SETTINGS はクライアントが CONNECT を送る前に ACK 済みの値、クライアントの SETTINGS はサーバーがレスポンスを送る前に ACK 済みの値がセッションの初期値になると規定する。`send_max` にはピアの広告値を使うべきだが、現状はローカル値を使っている。

`tokio-http2` の `accept()` では `overlay_settings(conn.local_settings())` でサーバー自身の SETTINGS を適用しており、これは `recv_max` 側として正しい。しかしピア（クライアント）の SETTINGS 値を `send_max` に反映する経路が欠落している。`ServerConnection::remote_settings()` は既に存在するため、既存 API の利用で対応可能。

## 設計方針

- `WtConfig` にピア SETTINGS 由来のフィールドを追加するのではなく、`WtSession::new` の引数としてローカル用 `WtConfig` とピア用 `WtConfig` を別々に渡す設計にする
- `WtFlowControl::new` のシグネチャを `(send_max, recv_max, max_streams_bidi_local, max_streams_bidi_remote, max_streams_uni_local, max_streams_uni_remote)` に変更する
- `WtStream::new` のシグネチャを `(id, send_max, recv_max, bidirectional)` に変更する
- ストリームレベルの `send_max` は開始主体によってピア SETTINGS の参照先が異なる:
  - ローカル開始 bidi ストリーム: ピアの `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE`（ピア視点で remote = ローカル開始）
  - ピア開始 bidi ストリーム: ピアの `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL`（ピア視点で local = ピア開始）
  - 単方向ストリーム: ピアの `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI`

## 完了条件

- ピアとローカルで異なるフロー制御値を設定した場合に、送信制限がピアの広告値に従うこと
- `tests/test_webtransport/integration.rs` に非対称なフロー制御値の単体テストが追加されていること

## 解決方法

1. `WtFlowControl::new` のシグネチャを変更し、`send_max`/`recv_max` と `max_streams_*_local`/`max_streams_*_remote` を別々に受け取る。`WtFlowControl::default()` の呼び出しも対応させる
2. `WtStream::new` のシグネチャを `(id, send_max, recv_max, bidirectional)` に変更する
3. `WtSession::new` でピア用 `WtConfig` を受け取り、`WtFlowControl::new` にピアの `initial_max_data` を `send_max` として渡す
4. `open_stream` でローカル開始ストリームの `send_max` にピアの `bidi_remote` / `uni` 値を使う
5. `handle_stream_data` でピア開始ストリームの `send_max` にピアの `bidi_local` / `uni` 値を使う
6. `tokio-http2` の `accept()` で `conn.remote_settings()` からピア用 `WtConfig` を構築し、`WtSession::server` に渡す
