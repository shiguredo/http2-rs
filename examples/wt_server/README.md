# wt_server

WebTransport over HTTP/2 エコーサーバーのサンプル実装。

draft-ietf-webtrans-http2-15 に対応し、Extended CONNECT (`:protocol=webtransport`) で確立されたセッション上で以下をエコーする。

- 双方向ストリーム: 受信データをそのまま返す
- 単方向ストリーム: 受信データを新しい単方向ストリームで返す
- DATAGRAM capsule: 受信ペイロードをそのまま返す

## 起動方法

デフォルト (`127.0.0.1:4443`) で起動する。

```bash
cd examples/wt_server
cargo run
```

リッスンアドレスを指定する。

```bash
cargo run -- --listen 127.0.0.1:4443
```

`WtServerRequest::reject(405)` の動作確認。

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
| `-l`, `--listen <ADDR>` | リッスンアドレス | `127.0.0.1:4443` |
| `--reject-connect` | 全セッションを 405 で拒否 | 無効 |
| `--allow-origin <ORIGIN>` | WebTransport CONNECT の `Origin` に許可する値。完全一致 (ASCII の大文字小文字は同一視) で比較し、一致しない場合は 403 を返す。`Origin` が欠落している場合は検証をスキップする | 検証しない |
| `-h`, `--help` | ヘルプを表示 | |
| `--version` | バージョンを表示 | |

## ログ

接続できない場合の切り分けに使う行を出力する。

起動時の行:

- `WebTransport (HTTP/2) server listening on https://<addr>`: 待ち受けを開始した
- `Certificate SHA-256 (base64): <hash>`: 自己署名証明書の SHA-256 ハッシュ (`serverCertificateHashes` 用)

接続ごとの行 (`[<remote>]` は接続元):

- `peer SETTINGS: <SETTINGS>`: ピアの SETTINGS。ピアが送っていない項目は既定値のまま表示される
- `WT CONNECT :protocol=<protocol>`: Extended CONNECT の `:protocol`
- `WT CONNECT authority=<authority> path=<path>`: リクエストの `:authority` と `:path`
- `WT CONNECT origin=<origin>`: `Origin`。欠落している場合は `WT CONNECT has no origin header` を警告として出力する

## 証明書

起動時に `rcgen` で自己署名証明書 (ECDSA P-256、有効期間 13 日) を自動生成し、SHA-256 ハッシュを base64 でログ出力する。
クライアントは証明書検証をスキップするか、出力されたハッシュを `serverCertificateHashes` 等で信頼する必要がある。

### キャッシュ

生成した証明書は `std::env::temp_dir()` 配下の `wt-server-http2-cert.jsonc` に JSONC 形式でキャッシュされる (例: macOS では `$TMPDIR/wt-server-http2-cert.jsonc`)。

- 残り有効期間が 1 時間以上あればキャッシュを再利用する
- 期限切れ・ファイル不在・パース失敗時は新規生成して上書き保存する

`serverCertificateHashes` で Chrome などに設定したハッシュを再起動後も使い回せるようにするための挙動。ハッシュを更新したい場合はキャッシュファイルを削除して再起動する。

## ALPN

TLS ALPN には `h2` を広告する。HTTP/3 版ではなく HTTP/2 上で WebTransport を扱う点に注意。
