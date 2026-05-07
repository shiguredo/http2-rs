# 変更履歴

- CHANGES
  - [UPDATE]: 後方互換がある変更
  - [ADD]: 後方互換がある追加
  - [CHANGE]: 後方互換のない変更
  - [FIX]: バグ修正

## develop

- [ADD] `shiguredo_http2` の依存に `bytes` (1.11, no_std + alloc 構成) を追加する
  - @voluntas
- [CHANGE] `shiguredo_http2` の Frame / HeaderField / Event / Capsule / WtEvent のバイト列ペイロードを `Vec<u8>` から `bytes::Bytes` に置き換えて relay 配信時の clone を Arc inc 化する
  - @voluntas
- [CHANGE] `shiguredo_http2::FrameDecoder` / `CapsuleDecoder` の内部バッファを `BytesMut` 化し、`split_to(payload).freeze()` でペイロードを zero-copy に切り出す
  - @voluntas
- [CHANGE] `shiguredo_http2::Connection::send_data` / `send_goaway`、`webtransport::WtSession::send_stream_data` / `send_datagram` / `poll_output` を `bytes::Bytes` ベースに変更する
  - @voluntas
- [CHANGE] `tokio-http2` の DATA フレーム送出 API (`Connection::send_data`, `Connection::send_goaway`, `ServerConnection::send_data`, `ClientConnection::send_data`) と WebTransport ストリーム / DATAGRAM API (`WtBidiStream::send` / `recv`, `WtUniSendStream::send`, `WtUniRecvStream::recv`, `WtServerSession::send_datagram` / `recv_datagram`) を `bytes::Bytes` ベースに変更する
  - @voluntas
- [ADD] `shiguredo_http2` の `Settings` に WebTransport 関連 SETTINGS (`0x2b61`〜`0x2b66`) を統合する
  - @voluntas
- [ADD] `shiguredo_http2::Limits` に `with_webtransport` ビルダーを追加する
  - @voluntas
- [ADD] `shiguredo_http2::Connection` に `local_settings` / `remote_settings` public アクセサを追加する
  - @voluntas
- [ADD] `shiguredo_http2::Event::HeadersReceived` に Extended CONNECT の `:protocol` 値を伝搬する `protocol: Option<Vec<u8>>` フィールドを追加する
  - @voluntas
- [ADD] `shiguredo_http2::webtransport::WtSession` に `send_max_data` / `send_max_stream_data` / `send_max_streams` / `grow_recv_window` / `grow_stream_recv_window` / `grow_max_streams` などの公開 API を追加する
  - @voluntas
- [ADD] `tokio-http2` に WebTransport over HTTP/2 サーバー API (`WtServerRequest`, `WtServerSession`, `WtSessionParts`, `WtSessionHandle`, `WtBidiStream`, `WtUniRecvStream`, `WtUniSendStream`) を追加する
  - @voluntas
- [ADD] `tokio-http2` で WebTransport セッションの動的フロー制御 (`WT_MAX_DATA` / `WT_MAX_STREAM_DATA` / `WT_MAX_STREAMS`) を自動発行する
  - @voluntas
- [ADD] `tokio-http2` で WebTransport DATAGRAM capsule の送受信を実装する
  - @voluntas
- [ADD] `examples/wt_server` を追加する (draft-ietf-webtrans-http2-14 対応のエコーサーバーサンプル)
  - @voluntas

### misc

- [ADD] `tokio-http2` に WebTransport 統合テスト (`tests/test_webtransport.rs`) を追加する
  - @voluntas
- [ADD] `issues/` ディレクトリと issue 運用を導入する
  - @voluntas
