# tokio-http2 の WebTransport 統合テストを追加する

- Created: 2026-04-17
- Model: Opus 4.7

## 概要

`crates/tokio-http2/tests/` に、WebTransport over HTTP/2 サーバーの動作を検証する統合テストを追加する。
クライアント側は tokio-http2 上に簡易 WT クライアントロジックを直書きする (外部依存は最小)。

## 背景

WT サーバー API (0003〜0006) は個別のユニットテストでは全体動作を検証しにくい。ループバック TCP 接続で実際のフレーム往復を確認する統合テストが必要。

## 根拠

- 0003〜0006 のどこか一つでも不備があるとサンプル (0008) がそもそも動かない
- 実配線のエコーを自動テストで担保することで、将来のリファクタリング安全性を確保する

## 対応内容

### テスト `crates/tokio-http2/tests/test_webtransport.rs`

- 同プロセス内で `Server::bind("127.0.0.1:0", ...)` を立て、ランダムポートでリッスン
- クライアントは同じ tokio-http2 の `Client` を使用し、Extended CONNECT を送信

- ケース:
  1. **bidi_echo**: 双方向ストリームでデータ往復
  2. **uni_echo**: クライアント→サーバー単方向 → サーバー→クライアント単方向でエコー
  3. **datagram_echo**: DATAGRAM capsule の往復
  4. **reject**: `WtServerRequest::reject(404)` を返し、クライアントが 404 レスポンスを受信すること
  5. **close**: `WtServerSession::close` で `WT_CLOSE_SESSION` を送出しクライアントが検知
  6. **drain**: `WtServerSession::drain` で `WT_DRAIN_SESSION` 後に新規ストリーム不可

### テストのファイル構成

- `crates/tokio-http2/tests/test_webtransport.rs`: メインファイル
- 長くなる場合は `mod` で分割

## 完了条件

- 上記 6 ケースが `cargo test -p tokio-http2` で通る
- `#[ignore]` を使わない
- 実装上の timing に依存しない安定したテスト

## 依存

- 0002〜0006 全て
