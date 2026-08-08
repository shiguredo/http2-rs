# no-content レスポンスへの空 DATA を違反として扱う実装と RFC の記述の不一致を修正する

- Priority: Medium
- Created: 2026-08-08
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-no-content-empty-data
- Polished: {YYYY-MM-DD}

## 目的

`Connection::handle_data` の no-content 違反チェック (`src/connection.rs` の `stream.no_content()` 分岐) は、204/304/HEAD レスポンスへの DATA フレームを **データ長を判定せず** すべて違反として RST_STREAM (PROTOCOL_ERROR) でリセットする。0 バイトの空 DATA (END_STREAM 通知のみ) も違反になる。

RFC 9113 Section 6.1 はゼロ長 DATA + END_STREAM をストリーム終端の合法的な手段として明記しており、RFC 9110 Section 6.4.1 の no-content は「コンテンツの不在」であり 0 バイト DATA はコンテンツを形成しない。相互運用性の観点で、空 DATA を違反としてリセットする現行実装は過剰応答のリスクがある。

また `src/connection/headers.rs` のコメントは「空 DATA は許容されるが、内容を持つ DATA は malformed として扱う」と書いており、実装との不一致がソースコード内に残っている。

## 現状

- `src/connection.rs` の `Connection::handle_data` は `if stream.no_content()` で `frame.data.len() == 0` を検査せず、空 DATA も含めて RST_STREAM (PROTOCOL_ERROR) でリセットする
- `src/connection/headers.rs` のコメントは「空 DATA は許容されるが、内容を持つ DATA は malformed として扱う」と書いており、実装と矛盾する
- RFC 9113 Section 6.1 (refs/rfc9113.txt) は「An endpoint that learns of stream closure after sending all data can close a stream by sending a STREAM frame with a zero-length Data field and the END_STREAM flag set」と、ゼロ長 DATA + END_STREAM を合法な終端手段として明記している
- RFC 9110 Section 6.4.1 の no-content (204/304/HEAD) は「コンテンツを含まない」ことであり、0 バイト DATA はコンテンツを形成しない

## 設計方針

no-content レスポンスへの DATA チェックにデータ長の条件を追加し、空 DATA (0 バイト) を許容する。内容を持つ DATA (1 バイト以上) のみを違反として RST_STREAM でリセットする。

- 条件: `stream.no_content() && !frame.data.is_empty()` のとき違反
- パディングのみの DATA (データ長 0 + パディング) の扱いは、フレームデコード後の `frame.data` が空になるため許容される (RFC 9113 Section 6.1 のフロー制御はパディングを含むペイロード全体に適用されるが、no-content チェックはコンテンツの有無のみを判定する)
- `src/connection/headers.rs` のコメントと実装の不一致は、コメントを実装に合わせて解消する (あるいは実装をコメントに合わせる)。設計方針どおり空 DATA を許容する場合は、コメントの記述が実装と一致する

## 完了条件

- no-content レスポンス (204/304/HEAD) への空 DATA (0 バイト) が許容され、`Event::DataReceived` が通知されること
- no-content レスポンスへの内容を持つ DATA (1 バイト以上) が従来どおり RST_STREAM (PROTOCOL_ERROR) + `Event::StreamReset` で処理されること
- `src/connection/headers.rs` のコメントと実装の不一致が解消されていること
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ること

## 解決方法

1. `src/connection.rs` の `Connection::handle_data` の no-content チェックにデータ長の条件を追加する
2. `src/connection/headers.rs` のコメントを実装と一致させる
3. `tests/test_connection.rs` に単体テストを追加する:
   - no-content レスポンスへの空 DATA (END_STREAM のみ) が許容されること (クライアントロール)
   - no-content レスポンスへの内容を持つ DATA が RST_STREAM (PROTOCOL_ERROR) で処理されること (既存の `test_no_content_violation_pushes_stream_reset` を参照)
4. `CHANGES.md` の `## develop` にエントリを追加する (shiguredo-changelog スキルを参照)

## 参照

- `refs/rfc9113.txt` — Section 6.1 (DATA) / Section 8.1.1 (Malformed Messages)
- `refs/rfc9110.txt` — Section 6.4.1 (Content)
- `src/connection.rs` — `Connection::handle_data` (no-content チェック)
- `src/connection/headers.rs` — no-content フラグ設定とコメント
- `src/stream.rs` — `Stream::no_content` / `Stream::set_no_content`
