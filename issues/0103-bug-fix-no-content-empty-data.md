# no-content レスポンスへの空 DATA を違反として扱う実装と RFC の記述の不一致を修正する

- Priority: Medium
- Created: 2026-08-08
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-no-content-empty-data
- Polished: 2026-08-08

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
- パディングのみの DATA (データ長 0 + パディング) の扱いは、フレームデコード後の `frame.data` が空になるため許容される (RFC 9113 Section 6.1 のフロー制御はパディングを含むペイロード全体に適用されるが、no-content チェックはコンテンツの有無のみを判定する)。ただしパディングのみ DATA は接続ウィンドウを `1 + pad_length` バイト消費するため、0102 の残課題 (パディング付き DATA のウィンドウ残留) の対象となる点に注意する
- no-content レスポンスは非ゼロ Content-Length を持つことが RFC 9113 Section 8.1.1 で合法とされており (「A response that is defined to have no content ... MAY have a non-zero content-length header field」)、ヘッダー処理側 (`src/connection/headers.rs`) には no-content の Content-Length チェック例外が実装済み。DATA 時点の Content-Length チェック (`src/connection.rs` の `handle_data` 内) にも no-content のスキップを追加し、HEAD + Content-Length: N + 空 DATA + END_STREAM が `received (0) != expected (N)` でリセットされないようにする
- `src/connection/headers.rs` のコメント「空 DATA は許容されるが、内容を持つ DATA は malformed として扱う」は既に設計方針どおりの内容であり、実装側をコメントに合わせる。`src/connection.rs` のコメント「(空 DATA を含む) は malformed として扱う」は実装と矛盾するため修正する
- 1xx (Informational) レスポンスは RFC 9110 Section 6.4.1 の no-content に含まれるが、1xx は END_STREAM を伴えず終端手段としての空 DATA が意味を持たないため、本 issue の対象外とする (現行の 204/304/HEAD のみの対象を維持する)

## 完了条件

- no-content レスポンス (204/304/HEAD) への空 DATA (0 バイト、END_STREAM の有無を問わず) が許容され、`Event::DataReceived` が通知されること
- no-content レスポンスへのパディングのみの DATA (データ長 0 + パディング) が許容されること
- no-content レスポンスへの内容を持つ DATA (1 バイト以上) が従来どおり RST_STREAM (PROTOCOL_ERROR) + `Event::StreamReset` で処理されること
- 非ゼロ Content-Length を持つ no-content レスポンス (例: HEAD + `Content-Length: N`) への空 DATA + END_STREAM が許容され、Content-Length チェックでリセットされないこと
- `src/connection.rs` のコメント (「(空 DATA を含む) は malformed として扱う」) と `src/connection/headers.rs` のコメントが実装と一致していること
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ること

## 他 issue との関係

- 0101 (`bug-fix-handle-data-stream-error`、closed): no-content 違反の扱い (空 DATA も含めて違反とする) を継承し、コメントと実装の不一致の解消は範囲外とされた。本 issue はその範囲外を引き取る
- 0102 (`change-connection-window-exhaustion`): 同じ `handle_data` の no-content 違反分岐 (`src/connection.rs` の `stream.no_content()` 内の `reset_stream` 呼び出し) を変更する。0103 は違反条件にデータ長の判定を追加し、0102 は同分岐内の `Event::StreamReset` に `connection_window_consumed` を反映する。意味論は互換 (0103 の非パディング空 DATA 許容は `flow_control_size == 0` でウィンドウ消費なし。ただしパディングのみ DATA は `1 + pad_length` バイト消費し、0102 の残課題 (パディング付き DATA のウィンドウ残留) の対象となる) だが、同一 if 文の条件と本文をそれぞれ変更するため機械的 merge では解決できず、実装順序の調整が必要。推奨順序は 0102 → 0103 (0102 実装後は同分岐の本文が `connection_window_consumed` 対応済みであることを前提に、0103 は条件に `&& !frame.data.is_empty()` を追加する)。`CHANGES.md` のエントリも競合する (内容は異なる箇所なので 3-way merge で解決できる見込み)

## 解決方法

1. `src/connection.rs` の `Connection::handle_data` の no-content チェックにデータ長の条件を追加する: `if stream.no_content() && !frame.data.is_empty()`
2. `src/connection.rs` の `handle_data` 内の Content-Length チェックに no-content のスキップを追加する (no-content レスポンスは非ゼロ Content-Length が合法のため、`stream.no_content()` の場合は Content-Length チェックをスキップする。ヘッダー処理側の実装と対称にする)。スキップ箇所に RFC 9113 Section 8.1.1 の「MAY have a non-zero content-length header field」を根拠とするコメントを追記する (ヘッダー処理側の対称の注記と揃える)
3. `src/connection.rs` の no-content チェックのコメント (「(空 DATA を含む) は malformed として扱う」) を、空 DATA は許容する内容に修正する。`src/connection/headers.rs` のコメントは既に設計方針どおりの内容であり変更不要
4. `tests/test_connection.rs` に単体テストを追加する:
   - no-content レスポンス (204) への空 DATA (END_STREAM 付き) が許容され、`Event::DataReceived` が通知されること (クライアントロール)
   - no-content レスポンスへの空 DATA (END_STREAM なし) が許容され、`Event::DataReceived` のみ通知されること
   - 非ゼロ Content-Length を持つ no-content レスポンス (HEAD + `Content-Length: N`) への空 DATA + END_STREAM が許容されること
   - no-content レスポンスへの内容を持つ DATA が従来どおり RST_STREAM (PROTOCOL_ERROR) + `Event::StreamReset` で処理されること (既存の `test_no_content_violation_pushes_stream_reset` がカバーしていることを確認し、退行しないこと)
   - パディングのみの DATA (データ長 0 + パディング) が no-content レスポンス (204、Content-Length なし) で許容されること
   - 304 / HEAD は 204 と同じ `set_no_content` 分岐 (クライアントロール) で処理されるため、204 の代表テストでカバーできることを確認する
5. `CHANGES.md` の `## develop` セクション内の既存 `[FIX]` 群の先頭に以下のエントリを追加する (リポジトリの慣習どおり新しいエントリを上に置く):

   ```markdown
   - [FIX] no-content レスポンス (204/304/HEAD) への空 DATA を違反として扱わず許容するように修正する。内容を持つ DATA のみを PROTOCOL_ERROR のストリームエラーで処理し、非ゼロ Content-Length を持つ no-content レスポンスへの空 DATA + END_STREAM も許容する (RFC 9113 Section 6.1 / Section 8.1.1 / RFC 9110 Section 6.4.1)
     - @voluntas
   ```

6. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ることを確認する

## 参照

- `refs/rfc9113.txt` — Section 6.1 (DATA) / Section 8.1.1 (Malformed Messages)
- `refs/rfc9110.txt` — Section 6.4.1 (Content) / Section 8.6 (Content-Length) / Section 9.3.2 (HEAD) / Section 15.3.5 (204) / Section 15.4.5 (304)
- `src/connection.rs` — `Connection::handle_data` (no-content チェックと Content-Length チェック)
- `src/connection/headers.rs` — no-content フラグ設定とコメント
- `src/stream.rs` — `Stream::no_content` / `Stream::set_no_content`
- `issues/closed/0101-bug-fix-handle-data-stream-error.md` — no-content 違反の扱いを範囲外とした経緯
- `issues/0102-change-connection-window-exhaustion.md` — 同じ `handle_data` の no-content 分岐を変更するため、実装順序の調整が必要

## 残課題 (本 issue のスコープ外)

- CONNECT 2xx (tunnel 確立) レスポンスは RFC 9110 Section 6.4.1 の no-content に含まれるが、現行実装では `no_content` が設定されず (204/304/HEAD のみ)、Content-Length 付きの CONNECT 2xx で tunnel データが超過すると PROTOCOL_ERROR でリセットされる。RFC 9110 Section 8.6 により CL 付き CONNECT 2xx は送信側 MUST NOT 違反であり実影響は小さいが、本 issue の対象外として明記する (別途対応)
