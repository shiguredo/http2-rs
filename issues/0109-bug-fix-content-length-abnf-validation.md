# Content-Length の ABNF 違反値 (符号付き数字) を許容する問題を修正する

- Created: 2026-08-10
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-content-length-abnf-validation
- Polished: {YYYY-MM-DD}

## 目的

`extract_content_length` が RFC 9110 Section 8.6 の ABNF (`Content-Length = 1*DIGIT`) に違反する値 (`+5` 等の符号付き数字) を `u64::parse` で受理してしまう。ABNF 外の値は RFC 9113 Section 8.1.1 の malformed であり、PROTOCOL_ERROR のストリームエラーで処理しなければならない (MUST)。0106 の修正で Content-Length パースエラー経路が RST_STREAM 変換されたことで、「パースは成功するが ABNF 違反」の値が malformed 検出を逃れる経路が残っている。

## 現状

`src/connection/headers.rs` の `extract_content_length` は `value_str.parse::<u64>()` で Content-Length をパースする。Rust の `u64::from_str` は先頭の `+` 記号を受理するため、`content-length: +5` のような ABNF 外の値が合法値として受理される (RFC 9110 Section 8.6 の ABNF は `1*DIGIT` であり、符号は含まれない)。`abc` のような数字以外の値は拒否されるが、符号付き数字は拒否されない。

## 設計方針

- パース前に値の全バイトが ASCII 数字 (`0-9`) であることを検証し、違反時は既存の `extract_content_length` と同じ `Error::stream_error(ErrorCode::ProtocolError, ...)` を返す
- 検証は `u64::from_str` の符号受理を排除する最小限の変更とする (RFC 9110 Section 8.6 準拠)
- 複数 Content-Length の不一致チェックは既存挙動を維持する

## 完了条件

- `content-length: +5` 等の符号付き数字が malformed として扱われ、PROTOCOL_ERROR のストリームエラーになる
- 正規の `content-length: 5` 等の ABNF 準拠値は従来どおり受理される
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `refs/rfc9110.txt` — Section 8.6 (Content-Length)
- `refs/rfc9113.txt` — Section 8.1.1 (Malformed Messages)
- `src/connection/headers.rs` — `Connection::extract_content_length`
