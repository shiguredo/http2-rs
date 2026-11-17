# shiguredo_nghttp2 の未生成 Error variant を削除する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/remove-unused-nghttp2-error-variants
- Polished: 2026-11-17

## 目的

`crates/shiguredo_nghttp2/src/error.rs` の `Error` には、本クレート内で一度も生成されない variant が 5 個残っている。CODEBASE.md の「公開 API は必ずテストで動作を保証すること」「既存の未使用・テスト未使用の公開 API を発見した場合は、テストを追加して動作を保証するか、削除して解消すること」に従い、テストで動作を保証できない未生成 variant を削除して CODEBASE.md を満たす。

## 現状

`crates/shiguredo_nghttp2/src/error.rs` の `Error` variant のうち、次の 5 個は `crates/shiguredo_nghttp2/src/` 内に構築箇所が無い (`rg` で確認済み)。

- `Error::InvalidArgument(String)`
- `Error::BufferTooSmall`
- `Error::SessionClosed`
- `Error::Callback(String)`
- `Error::Internal(String)`

`Error::Nghttp2` は `Error::from_nghttp2()` で、`Error::StreamNotFound` は `session.rs` の `get_stream_remote_window_size()` / `get_stream_local_window_size()` で構築される。`tokio-nghttp2` は `shiguredo_nghttp2::Error` を `Nghttp2(shiguredo_nghttp2::Error)` として包むだけで、削除対象の variant を直接 match していない。

## 設計方針

- `crates/shiguredo_nghttp2/src/error.rs` から上記 5 個の variant と、`Display` 実装の対応する arm を削除する。
- コードの挙動は変えない。削除対象の variant は生成されないため、既存のエラー経路に影響しない。
- `shiguredo_nghttp2` は `publish = false` かつバージョン `0.0.0` のクレートであり、公開 API の variant 削除は破壊的変更として扱う。
- issue 番号や issue への言及はソースコードに書かない (`shiguredo-issues` 規約)。

## 完了条件

- `crates/shiguredo_nghttp2/src/error.rs` の `Error` から `InvalidArgument` / `BufferTooSmall` / `SessionClosed` / `Callback` / `Internal` が削除されている。
- `Display for Error` から上記 variant の arm が削除されている。
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` が通る。
- `CHANGES.md` は CODEBASE.md の現行指示「この指示がなくなるまでは変更履歴を `CHANGES.md` に残さないこと」に従い、変更しない (`issues/0199-doc-update-changes.md` で CHANGES.md の運用と CODEBASE.md の整合を扱う)。

## 参照

- `crates/shiguredo_nghttp2/src/error.rs` — `Error` と `Display`
- `crates/shiguredo_nghttp2/src/session.rs` — `Error::Nghttp2` / `Error::StreamNotFound` の構築箇所
- CODEBASE.md — 公開 API の使用箇所とテスト
