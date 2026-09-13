# doc-test を実行するターゲットを追加し自動生成 bindings を除外する

- Created: 2026-09-13
- Completed: {YYYY-MM-DD}
- Branch: feature/add-doc-test-target
- Polished: {YYYY-MM-DD}

## 目的

doc-test を実行する手段を用意する。現状は `cargo test --workspace --doc` が `crates/nghttp2-sys/src/bindings.rs` の自動生成 doctest で失敗するため、`shiguredo_http2` の doctest (compile_fail を含む 14 件) を実行して確認する方法が無い。

## 現状

- `crates/nghttp2-sys/Cargo.toml` の `[lib]` は `doctest = false` を指定しており、`cargo test --workspace` では doctest は実行されない
- `cargo test --doc -p nghttp2-sys` は 8 件失敗する。`bindings.rs` は bindgen の自動生成ファイルで C のコード例を含み、rustdoc がそれを Rust として解釈するためである
- `cargo test --doc --workspace --exclude nghttp2-sys` は 14 件成功する (compile_fail の doctest を含む)
- `Makefile` に doc-test のターゲットは無く、`.github/workflows/ci.yml` も `--doc` を実行していない
- http3-rs の `Makefile` には `doc-test` ターゲットがあり、bindgen 生成クレートを `--exclude` して同じ問題を回避している

## 設計方針

- `Makefile` に `doc-test` ターゲットを追加し、`cargo test --doc --workspace --exclude nghttp2-sys` を実行する。除外する理由 (自動生成 bindings の C のコード例を rustdoc が Rust として解釈するため) をコメントで書く
- `.PHONY` に `doc-test` を追加する
- `.github/workflows/ci.yml` に doc-test のステップを追加する。追加する場合も同じ `--exclude` を使う
- 自動生成ファイル (`crates/nghttp2-sys/src/bindings.rs`) と `[lib] doctest = false` の設定は変更しない

## 完了条件

- `make doc-test` が成功すること
- `cargo test --doc --workspace --exclude nghttp2-sys` が成功すること (14 件)
- CI に doc-test のステップを追加した場合、3 ランナーで成功すること
- ライブラリのコードと自動生成ファイルに変更が無いこと
