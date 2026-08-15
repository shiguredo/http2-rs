# Content-Length の ABNF 違反値 (符号付き数字) を許容する問題を修正する

- Created: 2026-08-10
- Completed: 2026-08-15
- Branch: feature/fix-content-length-abnf-validation
- Polished: 2026-08-15

## 目的

`extract_content_length` が RFC 9110 Section 8.6 の ABNF (`Content-Length = 1*DIGIT`) に違反する値 (`+5` 等の符号付き数字) を `value_str.parse::<u64>()` で受理してしまう。

RFC 9110 Section 8.6 は ABNF に一致しない Content-Length の転送を MUST NOT で禁止している (request smuggling 対策)。RFC 9113 Section 8.2.1 の注記 (「Field values that are not valid according to the definition of the corresponding field do not cause a request to be malformed」) を踏まえると、ABNF 違反値を必ず malformed として扱う義務はないが、RFC 9110 Section 8.6 の ABNF との整合を実装判断として優先し、ABNF 違反値は既存の Content-Length パースエラー経路と同じ stream error (PROTOCOL_ERROR) で拒否する。

0106 の修正は Content-Length パースエラー経路の RST_STREAM 変換を対象としており、「パースは成功するが ABNF 違反」の値はエラー検出を逃れる経路として対象外のまま残っている。

## 現状

`src/connection/headers.rs` の `extract_content_length` は `value_str.parse::<u64>()` で Content-Length をパースする。Rust の `u64::from_str` は先頭の `+` 記号を受理するため、`content-length: +5` のような ABNF 外の値が合法値として受理される (RFC 9110 Section 8.6 の ABNF は `1*DIGIT` であり、符号は含まれない)。`abc` のような数字以外の値は拒否されるが、符号付き数字は拒否されない。

## 設計方針

- パース前に値の全バイトが ASCII 数字 (`0-9`) であることを検証し、違反時は既存の `extract_content_length` と同じ `Error::stream_error(ErrorCode::ProtocolError, ...)` を返す
- 検証は `u64::from_str` の符号受理を排除する最小限の変更とする (RFC 9110 Section 8.6 準拠)
- 複数 Content-Length の不一致チェックは既存挙動を維持する
- 本検証が拒否するのは ABNF 違反値のみであり、u64 に収まらない巨大な ABNF 準拠値の拒否やカンマ区切り同一値リストの拒否など、RFC 9110 Section 8.6 の MAY の範囲で実装された他の既存挙動は変更しない

## 完了条件

- `content-length: +5` 等の符号付き数字が ABNF 違反として拒否され、既存の Content-Length パースエラー経路 (`reset_headers_validation_error`) により RST_STREAM (PROTOCOL_ERROR) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成され、ストリームが `streams` から削除され、接続が維持される
- 正規の `content-length: 5` 等の ABNF 準拠値は従来どおり受理される
- 上記を検証する単体テストを `tests/test_connection.rs` の `mod reset_stream` (既存の `test_invalid_content_length_resets_stream_server` / `test_invalid_content_length_resets_stream_client` と同じ配置。0111 の分割実施後は分割先の該当サブモジュール) に追加し、`CHANGES.md` の `## develop` に `[FIX]` エントリ (shiguredo-changelog スキルに従う) を追加し、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `refs/rfc9110.txt` — Section 8.6 (Content-Length)
- `refs/rfc9113.txt` — Section 8.1.1 (Malformed Messages) / Section 8.2.1 (Field Validity)
- `src/connection/headers.rs` — `Connection::extract_content_length`

## 解決方法

1. `src/connection/headers.rs` の `extract_content_length` に、パース前に値の全バイトが ASCII 数字 (`0-9`) であることを検証する処理を追加した (空文字列は `1*DIGIT` の 1 桁以上に違反するため同様に拒否)。違反時は既存のパースエラーと同じ `Error::stream_error(ErrorCode::ProtocolError, ...)` を返し、`process_headers` の既存経路 (`reset_headers_validation_error`) により RST_STREAM (PROTOCOL_ERROR) 送信・`Event::StreamReset` (connection_window_consumed: 0) 生成・`streams` 削除・接続維持に変換される
2. `tests/test_connection.rs` の `mod reset_stream` に単体テスト 2 件 (`test_signed_content_length_resets_stream_server` / `test_signed_content_length_resets_stream_client`) を追加した。テスト値は `+0` を使い、END_STREAM 時の Content-Length 不一致チェック (非ゼロ値の検出) をすり抜けて ASCII 数字検証が唯一の拒否経路になることを検証する
3. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加した (shiguredo-changelog スキルに従う)
4. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ることを確認した
