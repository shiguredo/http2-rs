# examples/wt_server に自己署名証明書の JSONC キャッシュを追加する

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

`examples/wt_server/src/tls.rs` に、ECDSA P-256 自己署名証明書の JSONC キャッシュを追加する (http3-rs の `examples/wt_server` と同等の挙動)。

## 背景

現状は起動のたびに新規証明書を生成するため、Chrome などで `serverCertificateHashes` を使って接続する場合に SHA-256 ハッシュが毎回変わってしまい、クライアント側の設定を張り替える必要がある。

http3-rs の wt_server 同様、有効期間 13 日の間はキャッシュを再利用する運用にする。

## 根拠

- 「お手本」として http3-rs の wt_server と挙動を揃える
- Chrome の `serverCertificateHashes` は運用上、一定期間同じハッシュを再利用できる方が便利

## 対応内容

- `examples/wt_server/Cargo.toml` に `nojson = "0.3"` を追加
- `tls.rs` に以下を追加
  - `cache_path()`: `std::env::temp_dir().join("wt-server-http2-cert.jsonc")`
  - `load_cached_cert()`: JSONC を読み込み、残り有効期間 >= 1 時間ならキャッシュを使う
  - `generate_and_cache_cert()`: 新規証明書を生成し、`created_at` / `cert` / `key` を base64 で JSONC 保存
  - `generate_tls_server()` で 先に `load_cached_cert()` を試し、なければ生成
- README に証明書キャッシュの説明を追記

## 完了条件

- `cargo check -p wt_server` / `cargo clippy -p wt_server` が通る
- 1 回目の起動で JSONC を作成、2 回目の起動で同じ SHA-256 ハッシュが出る

## 依存

- 0008

## 解決方法

- `examples/wt_server/Cargo.toml` に `nojson = "0.3"` を追加
- `examples/wt_server/src/tls.rs` をキャッシュ対応に書き直し
  - `cache_path`: `std::env::temp_dir().join("wt-server-http2-cert.jsonc")`
  - `load_cached_cert`: `nojson::RawJson::parse_jsonc` で JSONC を読み、残り >= 1 時間ならキャッシュを返す
  - `generate_and_cache_cert`: 新規生成後に `created_at` / `cert` / `key` を base64 で書き込み
  - `generate_tls_server`: キャッシュを優先し、SHA-256 ハッシュを base64 でログ出力
- README.md にキャッシュ挙動の説明を追記
- `cargo check -p wt_server` / `cargo clippy --workspace --all-targets -- -D warnings` が通ることを確認
