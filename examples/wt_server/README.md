# wt_server

WebTransport over HTTP/2 エコーサーバーのサンプル実装。

draft-ietf-webtrans-http2-14 に対応し、Extended CONNECT (`:protocol=webtransport`) で確立されたセッション上で以下をエコーする。

- 双方向ストリーム: 受信データをそのまま返す
- 単方向ストリーム: 受信データを新しい単方向ストリームで返す
- DATAGRAM capsule: 受信ペイロードをそのまま返す

## 起動方法

デフォルト (`127.0.0.1:8443`) で起動する。

```bash
cd examples/wt_server
cargo run
```

リッスンアドレスを指定する。

```bash
cargo run -- --listen 127.0.0.1:8443
```

`WtServerRequest::reject(404)` の動作確認。

```bash
cargo run -- --reject-connect
```

ログレベルを変更する。

```bash
RUST_LOG=debug cargo run
```

## オプション

| オプション | 説明 | デフォルト |
| --- | --- | --- |
| `-l`, `--listen <ADDR>` | リッスンアドレス | `127.0.0.1:8443` |
| `--reject-connect` | 全セッションを 404 で拒否 | 無効 |
| `-h`, `--help` | ヘルプを表示 | |
| `--version` | バージョンを表示 | |

## 証明書

起動時に `rcgen` で自己署名証明書 (ECDSA P-256、有効期間 13 日) を自動生成し、SHA-256 ハッシュを base64 でログ出力する。
クライアントは証明書検証をスキップするか、出力されたハッシュを `serverCertificateHashes` 等で信頼する必要がある。

## ALPN

TLS ALPN には `h2` を広告する。HTTP/3 版ではなく HTTP/2 上で WebTransport を扱う点に注意。
