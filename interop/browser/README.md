# interop_browser

WebKit との WebTransport over HTTP/2 相互運用テスト

## 概要

実ブラウザ (Playwright の WebKit) が公開している WebTransport API を呼び出し、検証対象の WebTransport サーバー (`examples/wt_server`) が応答できることを確認するテストスイート。

Rust 側のテストでは再現できないブラウザ固有の要件 (Origin ヘッダー、`serverCertificateHashes`、TLS 1.3、ALPN `h2`) を確認するために使う。

## 検証対象

| 実装 | エンジン | 備考 |
|---|---|---|
| WebKit | Playwright の webkit | Safari 相当。Network.framework 経由で TCP 上の HTTP/2 にフォールバックする |

検証対象のサーバーは `examples/wt_server` のバイナリである。Chromium の HTTP/2 WebTransport 対応は未確認のため対象外である。

## 実行方法

```bash
cargo build -p wt_server
cd interop/browser
npm ci
npx playwright install webkit
node run.mjs
```

`node_modules` とブラウザが揃っていない環境では skip する。CI では必ず導入する。skip を失敗として扱いたい場合は `WT_FORCE=1` を設定する。

`run.mjs` は `examples/wt_server` を `--allow-origin <検証ページの Origin>` 付きで起動する (`WT_ORIGIN_MISMATCH=1` のときは不一致の Origin を渡す)。

Makefile からも実行できる。

```bash
make interop-test-browser
```

`make interop-test` (tokio-nghttp2 との疎通確認) には含まれない。npm とブラウザの導入に時間がかかるためである。

### 環境変数

| 変数 | 既定値 | 説明 |
|---|---|---|
| `WT_SERVER_BIN` | `target/debug/wt_server` | 検証対象のサーバーバイナリ |
| `WT_PORT` | `4443` | 検証対象のサーバーのポート |
| `WT_PAGE_PORT` | `0` | 検証ページのポート (0 は自動割り当て) |
| `WT_BROWSER_ENGINES` | `webkit` | 検証するエンジン (カンマ区切り) |
| `WT_FORCE` | 未設定 | `1` のとき Playwright やブラウザが無くても失敗させる |
| `WT_DUMP_SERVER` | 未設定 | `1` のとき終了時にサーバーログを出力する (失敗時は常に出力する) |
| `WT_ORIGIN_MISMATCH` | 未設定 | `1` のとき不一致の Origin を渡し、403 で拒否されることを確認する |

## 構成

| ファイル | 役割 |
|---|---|
| `run.mjs` | サーバー起動、証明書ハッシュの取得、検証ページ用証明書の生成、検証ページの配信、WebKit の駆動、判定 |
| `serve.mjs` | 検証ページを HTTPS で配信する |
| `index.html` | 検証ページ。WebTransport クライアントとして各項目を実行する |
| `package.json` / `package-lock.json` | 依存は `playwright` のみ (バージョン固定) |

検証ページ用の証明書は `certs/` に実行時生成する (`gitignore` 対象)。リポジトリに秘密鍵を置かないためである。生成には `openssl` を使う。ECDSA ではなく RSA を使うのは、http3-rs の実測で ECDSA の証明書が Node.js の TLS 実装に拒否されたためである。ページ側の証明書検証は Playwright の `ignoreHTTPSErrors` で無効化する。

## 検証項目

`index.html` の `checks` に定義する。各項目は独立した WebTransport セッションを開き、結果を `RESULT` 行として出力する。1 項目が失敗しても残りを続行する。

| 項目 | 内容 |
|---|---|
| `session` | セッション確立 (`transport.ready`) |
| `reliability` | `transport.reliability` が `reliable-only` であること (HTTP/2 へのフォールバックの確認) |
| `bidiEcho` | 双方向ストリーム 1 本のエコー (サーバーが送信側を閉じるまで読み切ってから、こちらの送信側を閉じる。理由は「送信側を閉じる順序」を参照) |
| `uniSend` | クライアント起点の単方向ストリーム送信 (ストリームの開設はサーバーログの `uni recv stream accepted` で確認できる。データの受信は `uniEcho` で確認する) |
| `datagrams` | datagram の送受信 (`createWritable()` または `writable` へ書き、`readable` からエコーを読み戻す) |
| `originRejected` | `WT_ORIGIN_MISMATCH=1` のときだけ実行し、セッションが確立しないこと (サーバーが 403 を返すこと) を判定する |

合否に含めない確認は `INFO` 行として出力する。

| 項目 | 内容 |
|---|---|
| `uniEcho` | ピア起点の単方向ストリーム (サーバーからのエコー) の受信 |
| `bidiEcho4KiB` | 4 KiB の双方向エコー (WebKit の H3 実装では 4 KiB 以上の書き込みが停止する実測があるため、HTTP/2 でも同じ制約が出るかを記録する) |

## 実測結果

- 確認環境: WebKit 26.6 (Playwright 1.63.0 の `webkit` エンジン、macOS 26.6.2 arm64)
- 確認日: 2026-09-14 (datagram は同日に再測定)
- 検証対象: `examples/wt_server` (`--allow-origin` に検証ページの Origin を指定)

| 項目 | 結果 |
|---|---|
| `session` | PASS (ready) |
| `reliability` | PASS (`reliable-only`) |
| `bidiEcho` | PASS (18 バイトのエコー) |
| `uniSend` | PASS (送信完了。サーバーログで `uni recv stream accepted` を確認できる) |
| `originRejected` | PASS (`WT_ORIGIN_MISMATCH=1` の実行でセッションが確立しない) |
| `datagrams` | PASS (14 バイトの送受信。使用した送信 API はログに記録する。HTTP/2 では再送されるため `reliability` は `reliable-only` のまま) |
| `uniEcho` (INFO) | 9 バイトのエコーが成功する (ピア起点の単方向ストリームを受信できる) |
| `bidiEcho4KiB` (INFO) | 4096 バイトのエコーが成功する (H3 の 4 KiB 停止は HTTP/2 では再現しない) |

### datagram の送信 API

WebKit 26.6 は仕様改訂後の `datagrams.createWritable()` を実装しており、旧仕様の `datagrams.writable` 属性は存在しない (`undefined` になる)。本検証は `createWritable()` があればそれを使い、無ければ `writable` を使う。

### 送信側を閉じる順序

`bidiEcho` と `uniEcho` は、送信側を閉じてからエコーを待つ実装だと約 10% の確率で `WebTransportError` (`source=session`) になるか、エコーが届かなくなることを実測している (現在の実装は 25 回連続実行して失敗 0)。

現在はストリームの終端まで読み切ってから、こちらの送信側を閉じている。

### WebKit が送る SETTINGS

実測値 (サーバーログの `peer SETTINGS` から値を抜粋したもの)。

| 設定 | 値 |
|---|---|
| `header_table_size` | 4096 |
| `enable_push` | false |
| `max_concurrent_streams` | 100 |
| `initial_window_size` | 2097152 |
| `max_frame_size` | 16384 |
| `enable_connect_protocol` | false |
| `max_header_list_size` | なし |
| `no_rfc7540_priorities` | true |
| `wt_enabled` | false |
| `wt_initial_max_data` | 8388608 |
| `wt_initial_max_stream_data_uni` | 8388608 |
| `wt_initial_max_stream_data_bidi_local` | 8388608 |
| `wt_initial_max_streams_uni` | 100 |
| `wt_initial_max_streams_bidi` | 100 |

`peer SETTINGS` は `Settings` の全フィールドを出力するため、ピアが送っていない項目は既定値のまま表示される。上表の値がすべて「ピアが送った値」とは限らない。`enable_connect_protocol` と `wt_enabled` は仕様上サーバーが送る設定であり、クライアントからは送られない (draft-ietf-webtrans-http2-15 Section 3.1)。

### CONNECT リクエスト

実測値 (サーバーログ)。

- `:protocol`: `webtransport` (一致しない場合サーバーは接続を閉じるため、セッション確立時点で一致が確認できる)
- `origin`: 検証ページの Origin (`https://127.0.0.1:<検証ページのポート>`) を送る
- `authority`: `127.0.0.1:4443`
- `path`: `/wt`
- 検証項目ごとに新しい接続を開く (接続は再利用されない)

## 既知の制約

- `--allow-origin` の一致判定は完全一致 (ASCII の大文字小文字は同一視) であり、末尾スラッシュの有無やポートの省略は一致しない。`WT_ORIGIN_MISMATCH` が確認するのはこの完全一致から外れた Origin が拒否されることである
- 検証項目は `:status = 200` と本文一致の範囲で判定し、フロー制御や複数ストリームの網羅は対象外である
- datagram の検証は 14 バイトを 1 個送受信するだけで、`maxDatagramSize` 付近の大きさ・複数個の順序・フロー制御は対象外である (HTTP/2 では受信側が datagram を破棄できるため、大量送信の検証には向かない)
- 双方向 / 単方向ストリームの検証はエコー本体の一致までを見る。サーバーが終端 FIN capsule を送るかどうかは判定していない (サーバーが終端 FIN capsule を送らなくてもクライアントの readable は終端する)
