# nghttp2-sys のビルドが brew の libngtcp2 等を参照してしまう

- Created: 2026-05-07
- Model: Opus 4.7

## 概要

`crates/nghttp2-sys` は `git clone` した nghttp2 を `cmake` で自前ビルドする設計だが、cmake が macOS の Homebrew にインストールされている `libngtcp2` などのオプショナル依存を勝手に検出してしまい、ビルドが失敗する場合がある。

`ENABLE_HTTP3=OFF` を指定しているにもかかわらず、nghttp2 の `CMakeLists.txt` は `find_package(Libngtcp2)` を無条件に呼ぶ。Homebrew 側の `libngtcp2` が見つかると `FindLibngtcp2.cmake` が `version.h` を読み取ろうとして失敗し、`Configuring incomplete, errors occurred!` で cmake が abort する。

## 再現手順

1. macOS で Homebrew 経由で `libngtcp2` をインストールしておく (`brew install libngtcp2` など)
2. クリーン状態から `cargo clippy --workspace` を実行
3. nghttp2-sys の build.rs (cmake 起動) で次のエラーで失敗:

```
CMake Error at cmake/FindLibngtcp2.cmake:21 (file):
  file STRINGS file
  "/opt/homebrew/Cellar/libngtcp2/1.20.0/include/ngtcp2/version.h" cannot be
  read.
```

## 根拠

`crates/nghttp2-sys` は外部システムの状態に依存しないために `git clone` + 静的ビルドを採用している。にもかかわらず Homebrew のライブラリを暗黙に拾ってしまうのは設計と矛盾する。HTTP/3 系の依存 (`Libngtcp2`, `Libngtcp2_crypto_*`, `Libnghttp3`) はそもそも `ENABLE_HTTP3=OFF` で不要であり、cmake の `find_package` を抑止すべき。

加えて、`Libev`, `Libevent`, `Libcares`, `Jansson`, `Jemalloc`, `Systemd`, `Libbrotlienc`, `Libbrotlidec` などのオプショナル依存も `ENABLE_LIB_ONLY=ON` のもとでは不要なので、まとめて検索を抑止する。

## スコープ

- `crates/nghttp2-sys/build.rs` の `cmake::Config` に `CMAKE_DISABLE_FIND_PACKAGE_*` を追加する

### 追加するフラグ

- `CMAKE_DISABLE_FIND_PACKAGE_Libngtcp2`
- `CMAKE_DISABLE_FIND_PACKAGE_Libngtcp2_crypto_quictls`
- `CMAKE_DISABLE_FIND_PACKAGE_Libngtcp2_crypto_libressl`
- `CMAKE_DISABLE_FIND_PACKAGE_Libngtcp2_crypto_wolfssl`
- `CMAKE_DISABLE_FIND_PACKAGE_Libnghttp3`
- `CMAKE_DISABLE_FIND_PACKAGE_Libev`
- `CMAKE_DISABLE_FIND_PACKAGE_Libevent`
- `CMAKE_DISABLE_FIND_PACKAGE_Libcares`
- `CMAKE_DISABLE_FIND_PACKAGE_Jansson`
- `CMAKE_DISABLE_FIND_PACKAGE_Jemalloc`
- `CMAKE_DISABLE_FIND_PACKAGE_Systemd`
- `CMAKE_DISABLE_FIND_PACKAGE_Libbrotlienc`
- `CMAKE_DISABLE_FIND_PACKAGE_Libbrotlidec`

## 受け入れ基準

- Homebrew に `libngtcp2` が入った macOS 環境でも `cargo clippy --workspace` が通る
- `cargo build --workspace` が通る
- `cargo test --workspace` が通る
