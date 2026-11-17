# CHANGES.md の draft-16 移行記載と CODEBASE.md の変更履歴禁止を整合させる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/update-changes
- Polished: 2026-11-17

## 目的

`CHANGES.md` の `## develop` には draft-16 への移行 (`SETTINGS_WT_ENABLED` の `0x2b60` から `0x2b67` への変更、`WT_MAX_DATA` / `WT_MAX_STREAM_DATA` / `WT_MAX_STREAMS` の厳密増加規則の変更) を記載したエントリがない。これらは後方互換のない wire 変更であり、`CODEBASE.md` の「この指示がなくなるまでは変更履歴を `CHANGES.md` に残さないこと」という凍結指示のために未記載のままになっている。既存の draft-15 参照エントリは、当時の仕様を記録した履歴として保全し、書き換えない (open 0118 の方針)。`CODEBASE.md` の凍結指示を削除して変更履歴の運用を `shiguredo-changelog` の基本方針に戻し、未記載の draft-16 移行を `## develop` に記録する。

## 現状

- `CHANGES.md` の `## develop` には 31 エントリがあり、draft-16 を参照するエントリは 0 件。draft-15 を参照するエントリが 21 件、draft 参照のないエントリが 10 件ある。draft-16 への wire 互換性変更を記載したエントリがない。既存の draft-15 参照は履歴として保全する。
- `### misc` が空のまま。
- `send_max_stream_data` のエントリは「現在の受信上限より小さい値は拒否」とだけ書き、実装の「同値も拒否」と一致しない。このエントリは draft-15 時点の 0155 の修正内容を記録したもので、同値拒否は draft-16 対応の 0166 で追加された後方互換のない挙動変更である。既存エントリは書き換えず、新しい `[CHANGE]` エントリで同値拒否を明記する。
- `CODEBASE.md` の凍結指示は 2026-09-13 に追加され、それ以降の変更は `CHANGES.md` に記録されていない。既存の 31 エントリは凍結前の履歴であり、指示との直接の矛盾ではない。draft-16 移行 (2026-11-03) が未記載のまま残っている点を解消する。

## 設計方針

- `CODEBASE.md` の「この指示がなくなるまでは変更履歴を `CHANGES.md` に残さないこと」を削除し、`shiguredo-changelog` の基本方針 (変更履歴は `CHANGES.md` に記載し、未リリースの変更は `## develop` に追記する) に戻す。既存の 31 エントリはそのまま維持する。
- `## develop` の既存 `[CHANGE]` 群の先頭に、次の draft-16 の wire 互換性変更を `[CHANGE]` エントリとして追加する。既存の draft-15 参照エントリは書き換えない。
  - `SETTINGS_WT_ENABLED` の識別子を draft-15 の `0x2b60` から draft-16 の `0x2b67` に変更したこと (draft-ietf-webtrans-http2-16 Section 11.2、0164)。
  - `WT_MAX_DATA` / `WT_MAX_STREAM_DATA` / `WT_MAX_STREAMS` を、初期値または最後に受理・広告した上限を増加させない値 (同値・減少) で拒否するようにしたこと (draft-ietf-webtrans-http2-16 Section 6.5 / Section 6.6 / Section 6.7、0165 / 0166 / 0167)。`send_max_stream_data` の同値拒否もこの `[CHANGE]` エントリに含める。
- 既存の `send_max_stream_data` の `[FIX]` エントリは、0155 時点の修正内容 (広告値の `recv_max` 反映と小さい値の拒否) を記録した履歴としてそのまま残す。同値拒否は新しい `[CHANGE]` エントリで説明する。
- 空の `### misc` を削除する。`## develop` の 31 エントリはいずれも機能変更であり、`### misc` に置くべき非機能変更がないため。
- `CODEBASE.md` の変更は `shiguredo-git` の特別規約に従い「整備」コミットとして `develop` に直接コミットし、`CHANGES.md` の変更は `feature/update-changes` で別コミットにする。`CHANGES.md` は `.md` の変更なので `shiguredo-changelog` の「.md ファイルの変更は変更履歴に反映しない」に従い、それ自体のエントリは追加しない。

## 完了条件

- `CODEBASE.md` に変更履歴の凍結指示が残っておらず、`CHANGES.md` の運用と矛盾しないこと。
- draft-16 の wire 互換性変更 (`SETTINGS_WT_ENABLED` の `0x2b67` 変更、`WT_MAX_*` の厳密増加規則) を記載した新しい `[CHANGE]` エントリが `## develop` にあること。
- 既存の draft-15 参照エントリが履歴として変更されていないこと。
- `send_max_stream_data` の同値拒否が新しい `[CHANGE]` エントリに明記され、`CHANGES.md` の記述が実装 (`src/webtransport.rs` の `send_max_stream_data` の `maximum <= advertised` 検証) と一致していること。
- 空の `### misc` が削除されていること。
- `cargo fmt --all -- --check` / `cargo test --workspace` はコード変更がないため不要。`CHANGES.md` と `CODEBASE.md` は prek の trailing-whitespace / end-of-file-fixer を通すこと。
