# interop/browser を新設し WebKit から WebTransport over HTTP/2 の疎通を確認する

- Created: 2026-09-13
- Completed: {YYYY-MM-DD}
- Branch: feature/add-webkit-browser-interop
- Polished: 2026-09-13

## 目的

実ブラウザから本実装 (`examples/wt_server` の tokio-http2) へ WebTransport over HTTP/2 で接続し、ブラウザ固有の要件を満たしているかを確認する。

http3-rs は `interop/browser` で Chromium / WebKit から H3 の WebTransport を検証し、Safari 固有の応答 SETTINGS 制約を実測で見つけている。HTTP/2 版にはこれに相当する検証が無く、Rust 側のテストでは再現できない要件 (Origin ヘッダー、`serverCertificateHashes`、TLS 1.3、`h2` ALPN) が未確認のままである。

本 issue では WebKit (Playwright の webkit エンジン) を対象に `interop/browser` を新設して疎通を確認し、結果を記録する。

## 現状

- WebKit の WebTransport は Network.framework の `nw_parameters_create_webtransport_http` を使う。`Source/WebKit/NetworkProcess/webtransport/cocoa/NetworkTransportSessionCocoa.mm` は `configureQUIC` と `configureTCP` の両方を渡し、QUIC が使えない場合は TCP 上の HTTP/2 にフォールバックする。`nw_webtransport_metadata_get_transport_mode` が `nw_webtransport_transport_mode_http2` のとき `WebTransportReliabilityMode` は `ReliableOnly` になる
- WebKit の H3 での WebTransport 動作は http3-rs で実測済みである (`docs/WEBKIT_WT.md`)。HTTP/2 での動作は未確認である
- HTTP/2 の WebTransport は unreliable delivery を提供しない。datagram は capsule として送受信でき、常に再送される (draft-ietf-webtrans-http2-15 Section 5.1)。したがって `transport.reliability` は `reliable-only` になり、datagram が「提供されない」わけではない。本実装も datagram を扱う (`WtSessionParts` の `datagram_rx` と `handle`、`examples/wt_server` の `run_echo`)
- `examples/wt_server` は自己署名証明書 (ECDSA P-256、有効期間 13 日) を生成し、SHA-256 ハッシュを base64 で `Certificate SHA-256 (base64): <値>` としてログ出力する。TLS の ALPN は `h2` で、`serverCertificateHashes` が要求する有効期間 14 日未満も満たす
- `examples/wt_server` の `handle_connection` は `WtServerRequest::accept(WtConfig::default(), None, None)` を呼び、`allowed_origin` に `None` を渡すため Origin の検証経路 (draft-ietf-webtrans-http2-15 Section 3.2) を通らない。ブラウザは `Origin` を送るため、実ブラウザでの検証ではこの経路を確認できない
- `WtServerRequest::accept` は `allowed_origin` に `Some` を渡すと Origin を検証し、一致しない場合は 403 を返す。ただし Origin が欠落している場合は検証をスキップして受け入れる (`crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_origin_missing_accepted` がこの挙動を固定している)。`examples/wt_server` のオプションは `--listen` と `--reject-connect` のみで、`--allow-origin` に相当するものは無い
- `Event::SettingsReceived` は `ack` しか運ばないため、ピア (WebKit) が送った SETTINGS の値は `ServerConnection::remote_settings()` を読まないと分からない。`examples/wt_server` は `remote_settings()` をログ出力していない
- 実ブラウザからの WebTransport 検証は存在しない。http3-rs の `interop/browser` は `WT_FORCE=1` を設定して CI の macOS ジョブから実行している
- 本リポジトリの CI は `schedule` (平日 1 回) のみで起動し、PR では実行されない。`timeout-minutes` は 15 分である (http3-rs は同じ内容のブラウザ検証を含むジョブを 30 分で実行している)
- WebKit (Network.framework) が実装している WebTransport over HTTP/2 の draft 版は未確認である。Safari の H3 実装が draft-07 と draft-13/14 のハイブリッドだった前例 (http3-rs の `docs/SAFARI_WT.md`) があり、本実装が準拠する draft-15 と差分がある可能性がある

## 設計方針

### 構成

`interop/browser/` を新設する。構成は http3-rs の `interop/browser` に倣う。

| ファイル | 役割 |
|---|---|
| `run.mjs` | `examples/wt_server` の起動、証明書ハッシュの取得、検証ページ用証明書の生成、検証ページの配信、Playwright の webkit 駆動、`RESULT` 行の集計、終了コードの決定 |
| `serve.mjs` | 検証ページを HTTPS で配信する |
| `index.html` | 検証ページ。`window.WT_CONFIG` (`url` / `certificateHash`) を受け取り、検証項目を実行して `RESULT` 行を console へ出す |
| `package.json` / `package-lock.json` | 依存は `playwright` のみ。バージョンは固定で指定する (`shiguredo-typescript` の「バージョン番号は固定して指定すること」) |
| `README.md` | 実行方法、検証項目、実測結果 |
| `.gitignore` | `node_modules/` / `.profile/` / `certs/` |

- 対象エンジンは WebKit のみとする (`WT_BROWSER_ENGINES` の既定値を `webkit` にする)。Chromium の HTTP/2 WebTransport 対応は未確認であり、本 issue の対象外とする
- 検証ページは接続先と同じ `127.0.0.1` で HTTPS 配信する。WebTransport は secure context を要求するためである
- 環境変数は `WT_SERVER_BIN` / `WT_PORT` / `WT_PAGE_PORT` / `WT_BROWSER_ENGINES` / `WT_FORCE` / `WT_DUMP_SERVER` / `WT_ORIGIN_MISMATCH` を用意する
  - `WT_DUMP_SERVER=1` のとき、終了時にサーバーログを stdout へ出力する (原因切り分け用)
  - `WT_ORIGIN_MISMATCH=1` のとき、検証ページと異なる Origin を `--allow-origin` に渡す
- skip の判定は `playwright` の解決可否とブラウザキャッシュの有無の両方を見る。`WT_FORCE=1` のときは skip を失敗として扱い、対象エンジンが未導入の場合も失敗にする (`npm ci` の失敗は skip の対象外で、そのままジョブの失敗とする)

### 証明書

- http3-rs は検証ページ用の証明書を `interop/browser/certs/` にコミットし、`run.mjs` が `readFileSync` で読み込む。本リポジトリでは `prek.toml` の `detect-private-key` フックが有効で PEM の秘密鍵をコミットできないため、`run.mjs` が起動時に生成して `certs/` (gitignore 対象) に置く
- 生成は `openssl req -x509 -newkey rsa:2048 -nodes` を `spawn` して行う。Node.js の標準モジュールだけでは X.509 証明書を生成できないためである。macOS runner には `/usr/bin/openssl` がある
- ページ配信の証明書は RSA を使う (http3-rs と同じ。同リポジトリでは ECDSA の証明書で Node.js の TLS ハンドシェイクが `decode error` になった実測がある)。ページ側の証明書検証は Playwright の `ignoreHTTPSErrors: true` で無効化する
- 接続先の証明書検証は `serverCertificateHashes` が担う。`examples/wt_server` 側の証明書が ECDSA P-256 かつ有効期間 13 日であることは現状のとおりでよい

### examples/wt_server への追加

- `--allow-origin <ORIGIN>` を追加し、`WtServerRequest::accept` の `allowed_origin` に渡す。実ブラウザの `Origin` に対する検証経路 (draft-ietf-webtrans-http2-15 Section 3.2) を確認するためである
- `handle_connection` で CONNECT を受信した後 (`WtServerRequest::from_connection` に `conn` を渡す前) に `ServerConnection::remote_settings()` の内容を `info` でログ出力する。WebKit が送る SETTINGS を記録し、接続できない場合の切り分けに使うためである。`Event::SettingsReceived` に値を載せる変更は公開 API の変更になるため本 issue の範囲外とする
- `examples/wt_server/README.md` のオプション表にも `--allow-origin` を追記する
- 証明書ハッシュのログ出力と、起動を表す `WebTransport (HTTP/2) server listening on` の行は `run.mjs` の待ち合わせに使う。これらの文字列は変更しない

### 検証項目

1 項目が失敗しても残りを続行し、結果を `RESULT PASS` / `RESULT FAIL` として出力する。4 KiB の確認だけは `INFO` として出力し、合否には含めない。

| 項目 | 内容 |
|---|---|
| `session` | セッション確立 (`transport.ready`) |
| `reliability` | `transport.reliability` が `reliable-only` であること (HTTP/2 へのフォールバックの確認) |
| `bidiEcho` | 双方向ストリーム 1 本のエコー (小さい payload)。あわせて 4 KiB の payload でも同じ手順を実行し、結果を `INFO` として記録する (http3-rs が WebKit で 4 KiB 以上の bidi 書き込みが停止することを実測しているため、HTTP/2 でも同じ制約が出るかを記録する) |
| `uniSend` | 単方向ストリームの送信 (サーバーは受信データを新しい単方向ストリームで返す) |
| `datagrams` | `transport.datagrams.writable` へ書き、`transport.datagrams.readable` からエコーを読み戻す。HTTP/2 では再送されるため `reliability` は `reliable-only` のままである (draft-ietf-webtrans-http2-15 Section 5.1) |
| `originRejected` | `WT_ORIGIN_MISMATCH=1` のときだけ実行し、セッションが確立しないこと (サーバーが 403 を返すこと) を判定する |

### 実測結果の記録

- 接続できない場合でも結果を残す。`RESULT FAIL` の内容と `WT_DUMP_SERVER=1` で取得したサーバーログから原因を切り分け、WebKit が送った `Origin` の値、`remote_settings()` の内容、`:protocol`、使用された transport mode を `interop/browser/README.md` に記録する
- 接続できた場合も、WebKit が送った `Origin` の値と `remote_settings()` の内容、`transport.reliability` の実測値を記録する。Origin が欠落していた場合は検証経路を通っていないため、その旨を明記する
- 本実装側の追従 (draft 版の差分への対応など) が必要な場合は、本 issue では対応せず別 issue を起票する

### 実行と CI

- Makefile に `interop-test-browser` を追加する。レシピは `cargo build -p wt_server` と `cd interop/browser && npm ci && npx playwright install webkit && node run.mjs` とする (`npm ci` は `package.json` のあるディレクトリで実行する必要がある)。`interop` ディレクトリの新設と `interop-test` は別 issue が扱うため、先に入っている場合は `.PHONY` とターゲットを既存の定義に追記する。npm とブラウザの導入に時間がかかるため `interop-test` には含めない
- CI の macOS ジョブ (`macos-26`) で実行する。`actions/setup-node` (コミットハッシュ固定 + バージョンコメント) で Node.js 22 を導入し、`npx playwright install webkit` の後に `node interop/browser/run.mjs` を `WT_FORCE=1` で実行する。Playwright 未導入を skip で緑にしないためである
- `.github/workflows/ci.yml` の `timeout-minutes` を 30 に引き上げる。Node.js の導入、`npm ci`、WebKit のダウンロード、ブラウザ実行を 15 分に収める根拠が無く、http3-rs は同じ内容のジョブを 30 分で実行しているためである
- `.github/workflows/ci.yml` の `on:` に `workflow_dispatch` を追加する。本リポジトリの CI は `schedule` のみで起動するため、追加したブラウザ検証を PR 上で実行して確認する手段が必要なためである
- Node.js スクリプトのコメントは日本語、ログメッセージは英語、テストのログメッセージは日本語にする (AGENTS.md)
- `CHANGES.md` は変更しない。`interop/browser` とその CI 手順の追加だけで、公開 API と配布物に影響しないためである (CODEBASE.md の「この指示がなくなるまでは変更履歴を `CHANGES.md` に残さないこと」にも従う)

## 完了条件

- `interop/browser` が新設され、`make interop-test-browser` で WebKit から `examples/wt_server` へ接続して `RESULT` 行が出力されること
- セッション確立・双方向ストリームのエコー・単方向ストリーム送信・datagram の送受信の成否が判定されること
- `transport.reliability` の実測値が `reliable-only` であること
- 4 KiB の bidi 書き込みの結果が `INFO` として記録されていること (合否には含めない)
- 実測結果 (成否、WebKit が送った `Origin` の値、`remote_settings()` の内容、`:protocol`、`transport.reliability`) が `interop/browser/README.md` に記録されていること
- `--allow-origin` に検証ページの Origin を渡した実行で `RESULT PASS session` になり、`WT_ORIGIN_MISMATCH=1` の実行で `RESULT PASS originRejected` になること (検証経路が生きていることの確認)
- 接続できなかった場合は、原因の切り分け結果と本実装側に必要な追従の有無が README に記録され、追従が必要な場合は別 issue が起票されていること
- `examples/wt_server` に `--allow-origin` が追加され、`remote_settings()` がログ出力されること。`examples/wt_server/README.md` のオプション表も更新されていること
- リポジトリに秘密鍵が追加されていないこと (ページ配信の証明書は実行時に生成する)
- CI の macOS ジョブに Node.js と WebKit の導入、および `node interop/browser/run.mjs` の実行が追加され、`timeout-minutes` が 30 に変更されていること
- `workflow_dispatch` による手動実行で CI のブラウザ検証が動作することを確認していること
- `cargo test --workspace` / `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` が通ること
