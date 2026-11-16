# `examples/wt_server` の証明書キャッシュが秘密鍵を共有 `/tmp` に既定権限で保存する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-server-key-cache
- Polished: {YYYY-MM-DD}

## 目的

`examples/wt_server` は自己署名証明書のキャッシュを `std::env::temp_dir()` 配下の固定名 `wt-server-http2-cert.jsonc` に保存する。このファイルには base64 化した秘密鍵が含まれる。Unix の共有 `/tmp` では umask 依存 (通常 0644) で他ユーザーから読め、固定名のシンボリックリンクを事前に置かれると書き込み先を追従する。キャッシュの生成・読み込みを安全にする。

## 現状

`examples/wt_server/src/tls.rs` の `generate_and_cache_cert` は `std::fs::write(&path, &jsonc)` で証明書と秘密鍵を書き込む。`cache_path()` は `std::env::temp_dir().join(CACHE_FILENAME)` を返す。`load_cached_cert` は所有者・パーミッション・証明書と鍵の対応・X.509 の `notAfter` を検証しない。

interop は `serverCertificateHashes` でこの証明書をピン留めするため、鍵の差し替えは現実的な影響を持つ。

## 設計方針

- 0700 のユーザー専用ディレクトリ (`XDG_RUNTIME_DIR` など) に移す。
- Unix では `OpenOptions` の `create_new` と `mode(0o600)`、`O_NOFOLLOW` 相当で作成する。一時ファイルに書いて `rename` する。
- 読み込み時に所有者・パーミッション・証明書と鍵の対応・有効期限を検証する。
- 秘密鍵をキャッシュしない運用も検討する。
- README にキャッシュの扱いと注意を書く。

## 完了条件

- キャッシュファイルが所有者のみ読み書き可能なパーミッションで作成されること。
- シンボリックリンクを追従しないこと。
- 読み込み時に所有者・パーミッション・証明書と鍵の対応・有効期限を検証すること。
- キャッシュの安全性を検証するテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
