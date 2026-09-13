# interop_h2

tokio-http2 と tokio-nghttp2 の疎通確認

## 概要

本実装 (`tokio-http2`) と nghttp2 C ライブラリの Tokio 統合 (`tokio-nghttp2`) の間で、HTTP/2 の疎通を検証するテストスイート。

網羅的な回帰テストではなく、実装の組み合わせが最小構成で疎通することを 1 コマンドで確認するための入口である。SETTINGS / フロー制御 / RST_STREAM などの網羅は `crates/tokio-http2/tests/interop.rs` が担う。

## テスト構成

```text
interop/h2/
  Cargo.toml                            -- クレート定義 (workspace メンバー)
  README.md                             -- このファイル
  src/lib.rs                            -- 共通ヘルパー (証明書生成・サーバー起動・1 往復)
  tests/
    nghttp2_client_http2_server.rs      -- nghttp2 クライアント ↔ 本実装サーバー
    http2_client_nghttp2_server.rs      -- 本実装クライアント ↔ nghttp2 サーバー
```

## テスト内容

各テストは双方向の GET 1 往復で次の 3 点を検証する。

- TLS で接続できること (両実装とも ALPN に `h2` を設定する。交渉結果は公開 API から取得できないため検証しない)
- `:status = 200` のレスポンスが返ること
- レスポンスボディが一致すること

WebTransport over HTTP/2 は対象外である。`tokio-nghttp2` は Extended CONNECT と WebTransport を提供しない。

## 依存ライブラリ

- `tokio-http2`: 本実装 (Sans I/O HTTP/2 コアの Tokio 統合)
- `tokio-nghttp2`: nghttp2 C ライブラリの Tokio 統合
- `rcgen`: テスト用の自己署名証明書の生成
- `rustls-pki-types`: 証明書の DER 型
- `tokio`: 非同期ランタイム

## テスト実行

```bash
# Makefile 経由 (interop のテストだけを実行する)
make interop-test

# パッケージ指定
cargo test -p interop_h2

# 個別のテストターゲット
cargo test -p interop_h2 --test nghttp2_client_http2_server
cargo test -p interop_h2 --test http2_client_nghttp2_server
```
