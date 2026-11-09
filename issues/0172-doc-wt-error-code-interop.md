# WT エラーコード暫定値の相互運用注記を精緻化する

- Created: 2026-11-09
- Completed: 2026-11-09
- Branch: feature/update-wt-error-code-interop
- Polished: 2026-11-09

## 目的

`ErrorCode::WtError` (0x100) / `ErrorCode::WtStreamStateError` (0x101) / `ErrorCode::WtFlowControlError` (0x102) が draft-ietf-webtrans-http2 では値未確定 (`0xTBD`) であり、IANA への登録も実施されていないことを確認し、この状態をすべての注記が同じ正確さで述べるようにする。draft-16 Section 11.3 の登録条項は 0xTBD のままなので、本実装の値は暫定値のまま維持することを結論とする。値が確定した draft が現れた場合に備え、注記に「確定時には後方互換のない (wire が変わる) 変更として値を更新する」ことを明示して、相互運用相手との差異の備えとする。

## 現状

- `refs/draft-ietf-webtrans-http2-16.txt` (draft 16、2026-11-02 投稿、datatracker 上の最新版) の Section 3.4 (`WT_ERROR (0xTBD)` / `WT_STREAM_STATE_ERROR (0xTBD)` / `WT_FLOW_CONTROL_ERROR (0xTBD)`) と Section 11.3 (登録条項の `Code: 0xTBD`) はいずれも値未確定
- IANA の HTTP/2 Error Code 登録表には WT 系の値は存在しない (0x0e 以降は Unassigned)。2026-11-09 に https://www.iana.org/assignments/http2-parameters/ で確認した
- `src/error.rs` の ErrorCode 列挙は、WebTransport 系 3 variant の直前の `//` 注記 (「draft-ietf-webtrans-http2-16 Section 11.3 では 0x100-0x102 の登録値が 0xTBD のため、これらは本実装の暫定値。IANA 登録後に更新される可能性がある。」) が唯一の暫定値説明であり、doc コメント (`///`) には暫定値の説明が無い
- `skills/shiguredo-http2/SKILL.md` の ErrorCode 表の下に暫定値の注記がある (`0x100`-`0x102` は draft-… が 0xTBD のため本実装の暫定値、という一文)
- `tests/test_error.rs` は暫定値の注記を持つ箇所が 2 箇所ある (既知コード一覧定数の直前の doc コメントと、mapping テスト関数の直前の doc コメント)。ファイルには `//!` による先頭 doc モジュールが存在しない
- `pbt/tests/prop_error.rs` は先頭の `//!` doc モジュールに暫定値の注記がある
- 注記の文言は各箇所で微妙に表現が異なり、「IANA 登録の現状」「値確定時の更新方針」は src/error.rs の注記にすら正確に述べられていない。「IANA 登録後に更新される可能性がある」という現行文言は、確定値の割当時期と更新の性格 (wire が変わる変更) を正確に表していない

## 設計方針

- 動作と値は一切変更しない。0x100-0x102 を維持する
- `src/error.rs` の暫定値注記 (`//` 実装コメント。shiguredo-rust 規約の仕様由来根拠コメントの分類に一致) を正として精緻化する。doc コメント (`///`) への変換は行わない。正の注記は次の 3 点 + 1 を述べる
  1. 0x100-0x102 は本実装の暫定値であり、draft-ietf-webtrans-http2-16 Section 11.3 では登録値が 0xTBD である
  2. IANA の HTTP/2 Error Code 登録表に WT 系の値はまだ無い (実装時に再確認し、確認した日付を issue の「解決方法」に記録する)
  3. draft 改訂や RFC 化で値が確定した時点で値を更新する。wire 値が変わる後方互換のない変更であるため、更新は利用者 (ピア実装者) への告知と一体で行う
- 他の注記 (`skills/shiguredo-http2/SKILL.md` の暫定値の文、`tests/test_error.rs` の 2 箇所の doc コメント、`pbt/tests/prop_error.rs` の先頭 doc モジュール) は、上の注記と「値・draft の状態・IANA の状態・更新方針」の事実が一致するように整える。全文一致を要求せず、表記の揺れがなく事実が漏れないことを基準とする。SKILL.md の注記は 1 文に別物説明 (`ErrorCode::WtError` と `webtransport::WtError` 構造体は別物) と暫定値説明が混在するため、別物部分は書き換えず暫定値部分のみ整える
- 衝突の照合: `refs/rfc9113.txt` Section 7 の既存登録値は 0x00-0x0d のみであり、0x100-0x102 とは衝突しない。この事実を注記には入れない (登録表側の値が増えるたびに注記が陳腐化するため。衝突の事実はコードレビュー時の一次資料照合で担保する)
- refs/ の draft は更新しない (CODEBASE.md の対応版は draft-16 のまま)。datatracker と IANA 登録表の確認日・結果は issue の「解決方法」に記録する

## スコープ外

- 0x100-0x102 の値変更、refs への draft 17 以降の取り込み
- WebKit 等を用いた実測相互運用試験
- `ErrorCode::WtError` の variant doc (src/error.rs) と SKILL.md の別物説明 (「別物」の文の部分) の見直し。現行のまま残す

## 完了条件

- `src/error.rs` の暫定値注記 (`//`) が、暫定値の理由 (draft 0xTBD・IANA 未登録・確定時は後方互換のない変更として更新) を漏れなく述べる
- `skills/shiguredo-http2/SKILL.md` (暫定値部分のみ) / `tests/test_error.rs` (2 箇所) / `pbt/tests/prop_error.rs` (先頭 doc モジュール) の注記が src/error.rs の注記と事実の一致・表記の一貫を満たす。別物説明部分は不変
- refs は draft-16 のまま変更されないこと、動作と値の変更がないこと
- datatracker での draft 最新版の確認と IANA 登録表の確認を実装時に再度行い、日付と結果を「解決方法」に記録する
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る


## 解決方法
- `src/error.rs` の暫定値注記 (`//`) を 5 行に精緻化した: 0x100-0x102 が本実装の暫定値であること、draft-ietf-webtrans-http2-16 Section 11.3 の登録値が 0xTBD であること、IANA の HTTP/2 Error Code 登録表に WT 系の値は存在しない (draft-16 時点) こと、値が確定した draft 改訂や RFC 化のタイミングで後方互換のない (wire 値が変わる) 変更として更新すること、更新は利用者への告知と一体で行うこと。doc コメント (`///`) への変換は行わず `//` のままとした (shiguredo-rust 規約の仕様由来根拠コメントの分類)
- `skills/shiguredo-http2/SKILL.md` の ErrorCode 注記は、別物説明の文を不変のまま残し、暫定値説明を独立の段落に分けた (行長 195 文字で MD013 の 200 以内に収める)。文言は src/error.rs の注記と同一の 4 事実を述べる
- `tests/test_error.rs` の 2 箇所の doc コメントと `pbt/tests/prop_error.rs` の先頭 doc モジュールに、IANA 登録未了 (括弧書きに統一) と「値が確定した時点では後方互換のない変更として更新される」を補足し、src/error.rs の注記と事実を揃えた
- 衝突照合の結果の注記への反映は行わない (issue の設計方針どおり)。refs 原本で RFC 9113 Section 7 が 0x00-0x0d までの登録値で 0x100-0x102 と衝突しないこと、RFC 8441 が HTTP/2 Error Code を登録しないことを確認した
- datatracker と IANA 登録表の再確認結果 (2026-11-09): draft-16 が 2026-11-02 投稿の最新版で draft 17 は未投稿。IANA HTTP/2 Error Code 登録表は 0x00-0x0d の実名登録のみで、0x0e 以降は Unassigned で WT 系の登録は未実施
- 値と動作の変更はなし (0x100-0x102 維持)。`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo doc -p shiguredo_http2 --no-deps` (警告は差分外の既存 7 件のみ) がすべて通ることを確認した

## 参照

- `src/error.rs` — `ErrorCode` 列挙と WT 系 variant 直前の暫定値注記 (`//`) / `from_u32` / `as_u32`
- `tests/test_error.rs` — 暫定値の注記を持つ 2 箇所の doc コメント (既知コード一覧定数と mapping テスト)
- `pbt/tests/prop_error.rs` — 先頭 doc モジュールの注記
- `skills/shiguredo-http2/SKILL.md` — ErrorCode 表の注記
- `refs/draft-ietf-webtrans-http2-16.txt` — Section 3.4 / Section 11.3 (0xTBD)
- `refs/rfc9113.txt` — Section 7 (HTTP/2 Error Code の登録値一覧。0x00-0x0d)
- IANA HTTP/2 Parameters registry: https://www.iana.org/assignments/http2-parameters/ (HTTP/2 Error Code 表。2026-11-09 時点で WT 系未登録を確認済み)
- datatracker: https://datatracker.ietf.org/doc/draft-ietf-webtrans-http2/ (draft 16 が 2026-11-02 投稿の最新版であることの 2026-11-09 確認)
- `issues/closed/0084-change-wt-error-code-names.md` — 暫定値維持の決着経緯
- `issues/closed/0169-wt-h2-draft-16-specification-references.md` — 暫定値注記を列挙部と SKILL.md / テストにまとめた経緯
