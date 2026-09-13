# interop/browser を新設し WebKit から WebTransport over HTTP/2 の疎通を確認する

- Created: 2026-09-13
- Completed: {YYYY-MM-DD}
- Branch: feature/add-webkit-browser-interop
- Polished: {YYYY-MM-DD}

## 目的

実ブラウザから本実装 (`examples/wt_server` の tokio-http2) へ WebTransport over HTTP/2 で接続し、ブラウザ固有の要件を満たしているかを確認する。

http3-rs は `interop/browser` で Chromium / WebKit から H3 の WebTransport を検証し、Safari 固有の応答 SETTINGS 制約を実測で見つけている。HTTP/2 版にはこれに相当する検証が無く、Rust 側のテストでは再現できない要件 (Origin ヘッダー、`serverCertificateHashes`、TLS 1.3、`h2` ALPN) が未確認のままである。

本 issue では WebKit (Playwright の webkit エンジン) を対象に `interop/browser` を新設して疎通を確認し、結果を記録する。

## 現状

- WebKit の WebTransport は Network.framework の `nw_parameters_create_webtransport_http` を使う。`Source/WebKit/NetworkProcess/webtransport/cocoa/NetworkTransportSessionCocoa.mm` は `configureQUIC` と `configureTCP` の両方を渡し、QUIC が使えない場合は TCP 上の HTTP/2 にフォールバックする。`nw_webtransport_metadata_get_transport_mode` が `nw_webtransport_transport_mode_http2` のとき `WebTransportReliabilityMode` は `ReliableOnly` になる
- WebKit の H3 での WebTransport 動作は http3-rs で実測済みである (`docs/WEBKIT_WT.md`)。HTTP/2 での動作は未確認である
- `examples/wt_server` は自己署名証明書 (ECDSA P-256、有効期間 13 日) を生成し、SHA-256 ハッシュを base64 で `Certificate SHA-256 (base64): <値>` としてログ出力する。TLS の ALPN は `h2` で、`serverCertificateHashes` が要求する有効期間 14 日未満も満たす
- `examples/wt_server` の `handle_connection` は `WtServerRequest::accept(WtConfig::default(), None, None)` を呼び、`allowed_origin` に `None` を渡すため Origin の検証経路 (draft-ietf-webtrans-http2-15 Section 3.2) を通らない。ブラウザは必ず `Origin` を送るため、実ブラウザでの検証ではこの経路を確認できない
- `WtServerRequest::accept` は `allowed_origin` に `Some` を渡すと Origin を検証し、一致しない場合は 403 を返す。`examples/wt_server` のオプションは `--listen` と `--reject-connect` のみで、`--allow-origin` に相当するものは無い
- 実ブラウザからの WebTransport 検証は存在しない。http3-rs の `interop/browser` は `WT_FORCE=1` を設定して CI の macOS ジョブから実行している
- WebKit (Network.framework) が実装している WebTransport over HTTP/2 の draft 版は未確認である。Safari の H3 実装が draft-07 と draft-13/14 のハイブリッドだった前例 (http3-rs の `docs/SAFARI_WT.md`) があり、本実装が準拠する draft-15 と差分がある可能性がある

## 設計方針

### 構成

`interop/browser/` を新設する。構成は http3-rs の `interop/browser` に倣う。

| ファイル | 役割 |
|---|---|
| `run.mjs` | `examples/wt_server` の起動、証明書ハッシュの取得、検証ページの配信、Playwright の webkit 駆動、`RESULT` 行の集計、終了コードの決定 |
| `serve.mjs` | 検証ページを HTTPS で配信する |
| `index.html` | 検証ページ。`window.WT_CONFIG` (`url` / `certificateHash`) を受け取り、検証項目を実行して `RESULT` 行を console へ出す |
| `package.json` / `package-lock.json` | 依存は `playwright` のみ。バージョンは固定で指定する |
| `README.md` | 実行方法、検証項目、実測結果 |
| `.gitignore` | `node_modules/` / `.profile/` / `certs/` |

- 対象エンジンは WebKit のみとする (`WT_BROWSER_ENGINES` の既定値を `webkit` にする)。Chromium の HTTP/2 WebTransport 対応は未確認であり、本 issue の対象外とする
- 検証ページは接続先と同じ `127.0.0.1` で HTTPS 配信する。WebTransport は secure context を要求するためである
- 環境変数は http3-rs に合わせて `WT_SERVER_BIN` / `WT_PORT` / `WT_PAGE_PORT` / `WT_BROWSER_ENGINES` / `WT_FORCE` を用意する。Playwright やブラウザが未導入の環境では skip し、`WT_FORCE=1` のときは失敗として扱う

### 証明書

- 検証ページの証明書はリポジトリに置かず、`run.mjs` が起動時に生成する。`prek.toml` の `detect-private-key` フックが有効であり、PEM の秘密鍵をコミットできないためである (http3-rs はページ用の証明書をコミットしているが、同リポジトリではフックを有効にしていない)
- ページ配信の証明書は RSA を使う。ECDSA の証明書は Node.js の TLS 実装が受け付けない (http3-rs の実測)
- 接続先の証明書検証は `serverCertificateHashes` が担う。`examples/wt_server` 側の証明書が ECDSA P-256 かつ有効期間 13 日であることは現状のとおりでよい

### examples/wt_server への追加

- `--allow-origin <ORIGIN>` を追加し、`WtServerRequest::accept` の `allowed_origin` に渡す。実ブラウザの `Origin` に対する検証経路 (draft-ietf-webtrans-http2-15 Section 3.2) を確認するためである
- `examples/wt_server/README.md` のオプション表にも `--allow-origin` を追記する
- 証明書ハッシュのログ出力と、起動を表す `WebTransport (HTTP/2) server listening on` の行は `run.mjs` の待ち合わせに使う。これらの文字列は変更しない

### 検証項目

1 項目が失敗しても残りを続行し、結果を `RESULT PASS` / `RESULT FAIL` として出力する。

| 項目 | 内容 |
|---|---|
| `session` | セッション確立 (`transport.ready`) |
| `reliability` | `transport.reliability` が `reliable-only` であること (HTTP/2 へのフォールバックの確認) |
| `bidiEcho` | 双方向ストリーム 1 本のエコー |
| `uniSend` | 単方向ストリームの送信 (サーバーは受信データを新しい単方向ストリームで返す) |
| `datagrams` | HTTP/2 では datagram が提供されない場合の挙動を記録する |

### 実測結果の記録

- 接続できない場合でも結果を残す。`RESULT FAIL` の内容とサーバーログから原因を切り分け、WebKit が送る SETTINGS と `:protocol`、使用された transport mode を `interop/browser/README.md` に記録する
- 本実装側の追従 (draft 版の差分への対応など) が必要な場合は、本 issue では対応せず別 issue を起票する

### 実行と CI

- Makefile に `interop-test-browser` を追加する (`cargo build -p wt_server` + `npm ci` + `npx playwright install webkit` + `node run.mjs`)。`interop` ディレクトリの新設と `interop-test` は別 issue が扱うため、先に入っている場合は `.PHONY` とターゲットを既存の定義に追記する。npm とブラウザの導入に時間がかかるため `interop-test` には含めない
- CI の macOS ジョブ (`macos-26`) で実行する。`actions/setup-node` (コミットハッシュ固定 + バージョンコメント) で Node.js 22 を導入し、`npx playwright install webkit` の後に `node interop/browser/run.mjs` を `WT_FORCE=1` で実行する。Playwright 未導入を skip で緑にしないためである
- Node.js スクリプトのコメントは日本語、ログメッセージは英語、テストのログメッセージは日本語にする (AGENTS.md)
- `CHANGES.md` は変更しない (CODEBASE.md の指示)

## 完了条件

- `interop/browser` が新設され、`make interop-test-browser` で WebKit から `examples/wt_server` へ接続して `RESULT` 行が出力されること
- セッション確立・双方向ストリームのエコー・単方向ストリーム送信の成否が判定されること
- `transport.reliability` の実測値が記録されていること
- 実測結果 (成否、失敗時の `RESULT FAIL` の内容とサーバーログ、WebKit が送る SETTINGS と `:protocol`) が `interop/browser/README.md` に記録されていること
- 接続できなかった場合は、原因の切り分け結果と本実装側に必要な追従の有無が README に記録され、追従が必要な場合は別 issue が起票されていること
- `examples/wt_server` に `--allow-origin` が追加され、ブラウザの `Origin` に対して 200 を返すこと。`examples/wt_server/README.md` のオプション表も更新されていること
- リポジトリに秘密鍵が追加されていないこと (ページ配信の証明書は実行時に生成する)
- CI の macOS ジョブに Node.js と WebKit の導入、および `node interop/browser/run.mjs` の実行が追加されていること
- `cargo test --workspace` / `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` が通ること
