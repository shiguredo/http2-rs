# doc-test を実行するターゲットを追加し自動生成 bindings を除外する

- Created: 2026-09-13
- Completed: {YYYY-MM-DD}
- Branch: feature/add-doc-test-target
- Polished: 2026-11-17

## 目的

doc-test だけを実行する Makefile ターゲットを用意する。`cargo test --workspace` は doctest も実行するが全テストを走らせるため、doctest だけを確認しづらい。また `cargo test --workspace --doc` は `crates/nghttp2-sys/src/bindings.rs` の自動生成 doctest で失敗するため、`--doc` だけを実行する専用の Makefile ターゲットが無い。

## 現状

- `crates/nghttp2-sys/Cargo.toml` の `[lib]` は `doctest = false` を指定しており、`cargo test --workspace` では nghttp2-sys の doctest は実行されない。他のクレートの doctest は実行され、`shiguredo_http2` の 16 件 (compile_fail 14 件、通常の doctest 2 件) は `cargo test --workspace` の doctest 実行で成功する
- `cargo test --doc -p nghttp2-sys` は 8 件失敗する。`bindings.rs` は bindgen の自動生成ファイルで C のコード例を含み、rustdoc がそれを Rust として解釈するためである
- `cargo test --doc --workspace --exclude nghttp2-sys` は 16 件成功する (compile_fail 14 件、通常の doctest 2 件)
- `Makefile` に doc-test のターゲットは無い。`.github/workflows/ci.yml` は `cargo test --workspace` で doctest を実行しているが、`--doc` 専用のステップは無い
- http3-rs の `Makefile` には `doc-test` ターゲット (`cargo test --doc --workspace`) がある。本リポジトリでは nghttp2-sys の自動生成 doctest を除外する必要があるため、`--exclude nghttp2-sys` を付ける

## 設計方針

- `Makefile` に `doc-test` ターゲットを追加し、`cargo test --doc --workspace --exclude nghttp2-sys` を実行する。除外する理由 (自動生成 bindings の C のコード例を rustdoc が Rust として解釈するため) をコメントで書く
- `.PHONY` に `doc-test` を追加する
- `.github/workflows/ci.yml` に doc-test のステップを追加し、`cargo test --doc --workspace --exclude nghttp2-sys` を実行する。`cargo test --workspace` は既に doctest を実行するが、他のテストが失敗すると doctest まで到達しないため、doc-test 専用のステップを追加する
- 自動生成ファイル (`crates/nghttp2-sys/src/bindings.rs`) と `[lib] doctest = false` の設定は変更しない

## 完了条件

- `make doc-test` が成功すること
- `cargo test --doc --workspace --exclude nghttp2-sys` が成功すること (16 件: compile_fail 14 件、通常の doctest 2 件)
- CI に doc-test のステップを追加し、3 ランナーで成功すること
- ライブラリのコードと自動生成ファイルに変更が無いこと
