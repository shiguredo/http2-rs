# examples/wt_server/ を実装する

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

http3-rs の `examples/wt_server/` に対応する HTTP/2 版の WebTransport エコーサーバーを `examples/wt_server/` に実装する。

## 背景

0002〜0007 で tokio-http2 に WebTransport サーバー API と統合テストが揃う。
これを使ったサンプルを用意することで、draft-14 の相互運用性を検証しやすくする。

## 根拠

- CLAUDE.md 「サンプルは **お手本** なので性能と堅牢性を両立させること」「サンプルは RFC に準拠していること」に従う
- HTTP/2 版 WebTransport の動作確認手段を提供する

## 対応内容

### ディレクトリ構成

```
examples/wt_server/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs
    ├── error.rs
    ├── tls.rs
    └── webtransport.rs  // 任意 (ラッパーが必要な場合)
```

### Cargo.toml

- `tokio-http2 = { path = "../../crates/tokio-http2" }`
- `rustls = "0.23"`, `rustls-pki-types = "1"`, `rcgen = "0.14"`
- `tokio = { version = "1", features = ["full"] }`
- `noargs = "0.4"`, `nojson = "0.3"`, `base64 = "0.22"`, `aws-lc-rs = "1"`, `time = "0.3"`
- `log`, `env_logger` (CLAUDE.md 「ログはできるだけださないが、使う場合は log を使うこと」)

### CLI (noargs)

- `-l, --listen <ADDR>` (デフォルト `127.0.0.1:8443`)
- `--reject-connect` (全セッションを 404 拒否)
- `-h, --help`, `--version`

### TLS

- http3-rs の `tls.rs` を HTTP/2 用にアレンジ
  - ALPN: `h2`
  - ECDSA P-256 自己署名、有効期間 14 日未満、SAN に localhost / 127.0.0.1 / ::1
  - JSONC でキャッシュ (`/tmp/wt-server-http2-cert.jsonc`)
  - SHA-256 ハッシュを base64 でログ出力 (serverCertificateHashes 用)

### エコー挙動

- 双方向ストリーム: 受信 → そのまま返す
- 単方向ストリーム: 受信 → 新規単方向ストリームで返す
- WT DATAGRAM: 受信 → そのまま返す

### README.md

- 起動方法、CLI オプション、証明書の扱いを記載 (http3-rs 版と同形式)
- HTTP/2 なので `h2` ALPN を使うこと、draft-14 準拠であることを明記

### ワークスペース扱い

- http3-rs の wt_server と同様にルート Cargo.toml の workspace には含めない (別ビルド対象)

## 完了条件

- `cd examples/wt_server && cargo run` でサーバーが起動
- 簡易クライアント (統合テストまたは canary.py のエコー版) でエコーが成立
- README.md が書かれており、実行手順が明記されている
- `cargo fmt` / `cargo clippy -D warnings` が通る

## 依存

- 0002〜0007 全て

## 解決方法

- `examples/wt_server/` 以下を新規追加
  - `Cargo.toml` (workspace に含める)
  - `src/main.rs`: CLI (`--listen`, `--reject-connect`)、サーバー起動、接続ごとに handle_connection
  - `src/tls.rs`: ECDSA P-256 自己署名証明書を生成し SHA-256 ハッシュをログ出力 (13 日有効期間)
  - `src/error.rs`: エラー型
  - `README.md`: 使い方
- ワークスペース設定 (ルート `Cargo.toml`) に `examples/wt_server` を追加
- `WtServerSession::into_parts()` で bidi / uni / datagram を `tokio::select!` で並行処理
- 単方向ストリームは `WtSessionHandle::open_uni()` で対向の送信ストリームを開いてエコー
- `cargo check -p wt_server` / `cargo clippy -p wt_server` がすべて通る
