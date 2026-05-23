# 変更履歴

- CHANGES
  - [UPDATE]: 後方互換がある変更
  - [ADD]: 後方互換がある追加
  - [CHANGE]: 後方互換のない変更
  - [FIX]: バグ修正

## develop

- [ADD] `HeaderField::from_static` を追加し、リテラル定数の RFC 違反 (大文字 field-name、CR/LF 含む値、未知の疑似ヘッダー、不正な `:status` 値など) を `const fn` 経由でコンパイル時に検出可能にする (issue 0024)
  - @voluntas
- [ADD] 構築時検査リファクタリング (issues 0024-0032) の Phase 1 として、新規エラー型 (`HeaderFieldError`, `FrameError`, `StreamIdError`, `SettingError`, `LimitsError`, `SendError`, `DecodeError`) と補助型 (`Parity`, `WindowSize`, `MaxFrameSize`, `WindowIncrement`, `Weight`, `LastStreamId`, `ClientStreamId`, `ServerStreamId`, `NonZeroStreamId`) を追加する (既存 API は無変更、Phase 2 で統合予定)
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
- [CHANGE] 旧コンストラクタ `HeaderField::new` (`fn(impl Into<String>, impl Into<String>) -> Self`) / `HeaderField::from_str` / `HeaderField::new_sensitive` / `HeaderField::sensitive`(コンストラクタ版) を廃止し、構築時検査つきの `HeaderField::new` (`Result<Self, HeaderFieldError>` 返却) と `HeaderField::new_with_sensitive` に置き換える (issue 0024)。アクセサ `HeaderField::sensitive() -> bool` は引き続き利用可能
  - @voluntas
- [CHANGE] `HeaderField` の全フィールドを private 化し、アクセサ `name() -> &[u8]` / `value() -> &[u8]` / `sensitive() -> bool` 経由でのみ読み取れるようにする (issue 0024)
  - @voluntas
- [CHANGE] `DynamicTable::insert` のシグネチャを `(impl AsRef<[u8]>, impl AsRef<[u8]>) -> Result<(), HeaderFieldError>` に変更し、構築時検査と整合させる (issue 0024)
  - @voluntas
- [CHANGE] `ValidationError` から個別フィールド値検査バリアント (`InvalidHeaderName` / `InvalidHeaderValue` / `InvalidMethodValue` / `InvalidSchemeValue` / `InvalidPathValue` / `InvalidStatusCode` / `InvalidProtocolValue` / `InvalidPseudoHeader`) を削除し、`InvalidHeaderField(HeaderFieldError)` / `DisallowedPseudoHeader` / `PseudoHeaderInTrailers` / `Status101NotSupported` に整理する (issue 0024)
  - @voluntas
- [CHANGE] `ValidationError::EmptyPath` の判定を scheme 依存に変更し、`:path` 空は http/https スキームのときのみ malformed として拒否する (RFC 9113 §8.3.1) (issue 0024)
  - @voluntas
- [CHANGE] `tokio_http2::WtServerRequest::reject(status)` で status を `100..=599` (RFC 9110 §15) に制限し、範囲外は `Err(Error::Io)` を返す (issue 0024)
  - @voluntas

### misc

- [UPDATE] `HeaderField::from_validated_parts` の cfg 排他 2 定義を解消し、テスト向け公開層を `__test_helpers::header_field_from_validated_parts` に集約する (issue 0033)
  - @voluntas
- [ADD] `tokio-http2` に WebTransport 統合テスト (`tests/test_webtransport.rs`) を追加する
  - @voluntas
- [ADD] `issues/` ディレクトリと issue 運用を導入する
  - @voluntas
- [ADD] PBT / fuzz クレートから `HeaderField::from_validated_parts` および crate 内部の const fn / runtime 検査関数の panic-catch ラッパを呼ぶための cargo feature `__test_helpers` を追加する (本番利用者は有効化禁止、型不変条件を破壊する) (issue 0024)
  - @voluntas
- [ADD] HPACK 構築時検査の const fn 版と runtime 版の同値性プロパティテスト (`pbt/tests/prop_header_field_syntax.rs`) を追加する (issue 0024)
  - @voluntas
- [CHANGE] issue 0013 (HTTP/2 ペイロードを Bytes 化する) を `bytes` クレート依存追加の保留に伴い `issues/pending/` に退避する
  - @voluntas
