# draft の値確定時に WT エラーコード暫定値を更新する

- Created: 2026-11-10
- Completed: {YYYY-MM-DD}
- Branch: feature/update-wt-error-code-value
- Polished: {YYYY-MM-DD}

## 目的

WebTransport over HTTP/2 の HTTP/2 Error Code (`WT_ERROR` / `WT_STREAM_STATE_ERROR` / `WT_FLOW_CONTROL_ERROR`) は、登録値が draft で `0xTBD` (IANA 未登録) の間、本実装の暫定値 `0x100`-`0x102` で運用する (closed 済みの `issues/closed/0172-doc-wt-error-code-interop.md` 参照)。本 issue は、draft 改訂または RFC 化で最終値が確定した (0xTBD が解消した) ことを観察した時点で、暫定値を確定値に更新する作業を追跡する。相互運用する関連実装も暫定値 `0x100`-`0x102` を共有する運用であるため、値が確定した draft が現れた場合に相互運用差分を観察する起点にもなる。

## 現状

- `refs/draft-ietf-webtrans-http2-16.txt` (draft 16、2026-11-02 投稿) の Section 3.4 と Section 11.3 はいずれも `0xTBD`。IANA の HTTP/2 Error Code 登録表に WT 系の値は存在しない (0x0e 以降 Unassigned)。2026-11-09 時点で draft 16 が datatracker 上の最新版であることを確認済み (closed issue 0172)。
- 2026-11-17 時点でも draft 16 が datatracker 上の最新版であり、IANA 登録表も 0x0e 以降 Unassigned のままであることを再確認した。
- `src/error.rs` (`ErrorCode` 列挙 / `from_u32` / `as_u32`) と `skills/shiguredo-http2/SKILL.md`、`tests/test_error.rs` (既知コード一覧定数、mapping テスト、未知コード範囲のテスト)、`pbt/tests/prop_error.rs` (先頭 doc モジュールと `is_known` の既知コード範囲) が暫定値 `0x100`-`0x102` で一致しており、mapping テストと未知コード範囲のテストが wire 値の参照ピンとして機能する。
- 相互運用側の実装も同じ暫定値を用いることを 2026-11-10 に確認済み。相手側のテストは wire 値の固定を外し、値の共有は両実装間で整合済み。wire 値の分岐が起きた場合の差別化検討は相互運用側の責務であり、本リポジトリは draft の登録値に追従する。

## 設計方針

- draft の改訂・IANA 登録の観察は datatracker と IANA 登録表で行い、draft 16 以降の新規版が出たら本 issue で `Section 3.4` / `Section 11.3` を確認する。
- 値が確定した draft / RFC を観察した時点で、`/update-refs` により `refs/` を確定版に差し替え (ユーザー承認)、`/refresh-issue` で本 issue を現状に同期する。値が未確定の間は refs を差し替えず draft-16 のままとする (スコープ外の他の改訂を取り込まない)。
- 値が確定したことを観察したら、`src/error.rs` の割当 (variant 値と `from_u32` / `as_u32`)、`skills/shiguredo-http2/SKILL.md`、`tests/test_error.rs` (既知コード一覧、mapping テスト、未知コード範囲のテスト)、`pbt/tests/prop_error.rs` の注記と既知コード範囲 (`is_known`) を実装値に更新する。
- 注記内の draft の版表記は、値の根拠となる確定版 (draft 17 以降または RFC) に合わせる。値以外の draft 節参照の全面更新は別 issue に委ねる (スコープ外参照)。
- 値が変わる場合、wire 値が変わる後方互換のない変更であるため、変更コミットは利用者 (ピア実装者) への告知と一体で進める (closed issue 0172 の更新方針を踏襲)。値が確定するまで現行の暫定値を変えない。
- 確定した値が現行の暫定値 `0x100`-`0x102` と同一だった場合も「確定値と一致したこと」を本 issue の解決方法に記録し、注記の文言を「暫定値が確定値と一致した」旨に整えて終了とする (実装面の値変更なし)

## スコープ外

- 値が未確定の間の draft 改訂自体の取り込み (値が未確定の間は refs を draft-16 のまま固定し、値が確定した draft / RFC が出た時点で確定版に差し替える)
- 相互運用側の値の変更やテスト構成の変更
- draft の他の改訂 (`Settings_WT_*` / capsule / ESTABLISHMENT 手順等) の取り込み (改訂差分の精査は、当該時点の open issue として個別に起票する)
- `CODEBASE.md` / `README.md` / `SKILL.md` の対応版や、値以外の draft 節参照の全面更新 (draft 全体の同期は別 issue で行う)

## 完了条件

- draft が値を確定させる改訂または RFC 化を観察したとき、[refs](refs/draft-ietf-webtrans-http2-16.txt) を確定版に差し替え (ユーザー承認)、code / SKILL.md / tests / pbt の暫定値を確定値に更新する。値が現行の暫定値と一致した場合は、その旨を解決方法に記録し、注記を一致した旨に整える (実装面の値変更なし)
- ワイヤ互換性と相互運用差分を本 issue の解決方法に記録する (相互運用側の意図も含め、確認日と参照した draft の版を記録)
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` を通る

## Pending 理由

2026-11-17 時点で、draft-ietf-webtrans-http2-16 が datatracker 上の最新版であり、Section 3.4 と Section 11.3 の WT エラーコードは `0xTBD` のままである。IANA の HTTP/2 Error Code 登録表にも WT 系の値は登録されておらず、0x0e 以降は Unassigned である。したがって、暫定値 `0x100`-`0x102` を確定値へ更新する作業は、値が確定した draft 改訂または RFC の公開まで着手できない。

値が確定した draft / RFC が公開された時点で本 issue を reopened にし、`Section 3.4` / `Section 11.3` を確認して `refs/` の差し替え (ユーザー承認) と code / SKILL.md / tests / pbt の更新を行う。値が未確定の間は、refs を draft-16 のまま固定し、暫定値 `0x100`-`0x102` を変更しない。

## 参照

- `issues/closed/0172-doc-wt-error-code-interop.md` — 暫定値注記の精緻化と ongoing 観察の起点
- `issues/closed/0169-wt-h2-draft-16-specification-references.md` — draft 版参照の同期の前例
- `src/error.rs` — `ErrorCode` / `from_u32` / `as_u32`
- `tests/test_error.rs` / `pbt/tests/prop_error.rs` — wire 値の参照ピン
- `refs/draft-ietf-webtrans-http2-16.txt` — Section 3.4 / Section 11.3
- datatracker: https://datatracker.ietf.org/doc/draft-ietf-webtrans-http2/ (新規版の観察)
- IANA HTTP/2 Parameters registry: https://www.iana.org/assignments/http2-parameters/ (HTTP/2 Error Code 表)
