# refs/ の WebTransport draft を最新版に更新し、ソースコメントの draft 番号を同期する

- Priority: Medium
- Created: 2026-06-11
- Polished: 2026-06-12
- Model: deepseek-v4-pro
- Branch: feature/change-update-refs-draft-15

## 目的

`skills/shiguredo-http2/SKILL.md` が draft-15 を参照している一方、`refs/` には draft-14 が存在し、全ソースコードコメントも draft-14 を参照しているバージョン不一致を解消する。

本 issue は **refs/ ファイルの差し替えと、ソースコメント / SKILL.md / `Cargo.toml` 等の draft 番号テキストを最新版に機械的に置換する** ことまでをスコープとする。**仕様変更に伴う実コード修正 (Capsule Type 値の wire 変更、SETTINGS Identifier 変更、新規 MUST 要件の追加対応等) は本 issue では行わず、`update-refs` 実行後に diff を確認したうえで個別の `[CHANGE]` issue として分解起票する**。

## 優先度根拠

- `skills/shiguredo-http2/SKILL.md` (15 行目、170, 222, 414, 487, 496 等) が `draft-ietf-webtrans-http2-15` を参照しているが、実装の根拠は `refs/draft-ietf-webtrans-http2-14.txt`。スキルとリポジトリ実態の不一致は LLM ベースのレビューや他 issue の polish 工程で誤情報を伝播する経路となる
- draft-14 → 最新 (draft-15 以降) で wire レベルの仕様変更 (Capsule Type 値、SETTINGS Identifier、エラーコード、フロー制御要件) があった場合、未追従のままリリースすると相互運用性問題を抱える。本 issue で diff を取得することで、後続 issue 群の起票根拠を確立する
- 修正コスト自体は低い (`update-refs` でファイル差し替え、テキスト一括置換、検証実行)。仕様追従のコストは本 issue のスコープ外として後続 issue で扱う

## 現状の問題

- `refs/draft-ietf-webtrans-http2-14.txt` が現状のリポジトリ内 draft 本文
- `skills/shiguredo-http2/SKILL.md:15` 他は `draft-ietf-webtrans-http2-15` を参照
- ソースコード・テスト・examples・README・`Cargo.toml` 等で `draft-ietf-webtrans-http2-14` への言及が多数 (`grep -rn "draft-ietf-webtrans-http2-14" src/ crates/ tests/ pbt/ examples/` で確認、想定 100 箇所以上)
- 2026-06-12 時点の IETF Datatracker では `draft-ietf-webtrans-http2` の最新版は `draft-ietf-webtrans-http2-14` であり、`draft-ietf-webtrans-http2-15` は存在しない。そのため、最新版確認時に引き続き draft-14 が最新である場合は refs/ とソースコメントを draft-15 へ更新してはならない

バージョン不一致により:

- スキル経由のレビューや指示が「最新 draft に対応」と告げるが実装は draft-14 のまま
- draft-15 以降で仕様が変更された場合、未追従の問題箇所を特定する手段がない (diff が手元にないため)
- `WtErrorCode::WebtransportError = 0x100` 等の暫定値、`Capsule::WT_*` の暫定 Capsule Type 値、`SETTINGS_WT_*` の暫定 SETTINGS Identifier (0x2b61〜0x2b66) が draft-15 以降で IANA 登録または値変更されている可能性がある

## 設計方針

- `update-refs` スキルを実行し、IETF Datatracker から **最新版** の `draft-ietf-webtrans-http2` を取得する
- 取得した最新版のバージョンを確定する。最新版が draft-15 以降であれば、refs/ の draft-14 ファイルを最新版に差し替える
- 最新版が draft-14 のままであれば、refs/ とソースコメントは既に最新版と一致しているため差し替えない。この場合の不一致は `skills/shiguredo-http2/SKILL.md` 側の draft-15 記述が誤っている問題として扱い、本 issue の実装作業は停止してユーザーに報告する
- draft-15 以降へ更新する場合のみ、ソースコメント・テスト・examples・README の draft 番号テキスト (`draft-ietf-webtrans-http2-14`) を取得した最新版番号に **機械的に置換**する。意味的な仕様追従は本 issue では行わない
- draft-14 と最新版の diff を取得し、本 issue 完了時のレポートとして残す。diff 内容に基づいて後続の `[CHANGE]` issue を起票する判断は本 issue のスコープ外で、ユーザー / 別 polish 工程で行う

## スコープ外

- **仕様変更に伴う実コード修正**: Capsule Type 値の wire 変更、SETTINGS Identifier 変更、エラーコード値変更、新規 MUST/SHOULD 要件の追加対応、WT_CLOSE_SESSION reason 上限の変更追従、WebTransport-Init ヘッダーフォーマット変更追従等は **本 issue では行わない**。`update-refs` 実行後の diff を確認したうえで個別の `[CHANGE]` issue (例: `change-follow-draft-15-capsule-type-iana`) として分解起票する
- **過去 CHANGES.md エントリ・closed issues・refs 原本本文の置換は禁止**:
  - `CHANGES.md` の `## バージョン` 配下にある過去リリース履歴の draft-14 言及 (歴史的事実) は置換しない
  - `issues/closed/` 配下の過去 issue ファイル内の draft-14 言及 (歴史的事実) は置換しない
  - `refs/draft-ietf-webtrans-http2-14.txt` 本文の `Internet-Draft draft-ietf-webtrans-http2-14` は置換不要 (ファイル自体を最新版に差し替える)
- SKILL.md の draft-15 表記が draft-15 (取得した最新版が draft-15 と一致する場合) であれば修正不要。最新版が draft-16 以降だった場合は SKILL.md 全体を最新版番号に揃える
- `pbt/tests/prop_webtransport/main.rs` 内の Capsule Type 数値 (`0x190B4D3D` 等) の hex 値変更は本 issue スコープ外 (上記の分解起票で扱う)

## 他 issue との関係

- **0068 (`bug-fix-wt-error-design`)**: `src/webtransport/error.rs` の draft-14 コメントに本 issue が触れる可能性があり、0068 マージ後にマージするのが安全
- **0070 (`change-privatize-error-wt-error-fields`)**: 同上、`src/webtransport/error.rs` への影響あり。0070 マージ後にマージする
- **0072 (`refactor-remove-unused-code`)**: `src/webtransport/error.rs` / `src/webtransport/flow_control.rs` への影響あり。0072 マージ後にマージする
- **0073 (`change-rfc9297-allow-non-minimal-varint`)**: `src/webtransport/varint.rs` のコメント書き換えに触れる。0073 マージ後にマージする。本 issue で draft-14 ファイルが削除されるため、0073 の「参照」セクションに残る draft-14 への参照は本 issue 内で同時に最新版番号に置換する
- **0075 (`fmt-replace-unwrap-with-expect`) / 0076 (`fmt-translate-english-comments`)**: それぞれ無関係
- **0065 / 0066 (open)**: WebTransport モジュール本体を触る issue で、本 issue および意味追従群の後に着手するのが安全
- **draft-15 意味追従（本 issue のスコープ外として分解起票済み）**:
  - **0081** `change-settings-wt-enabled` — `SETTINGS_WT_ENABLED` (0x2b60)
  - **0082** `change-wt-stream-fin-polarity` — WT_STREAM FIN 極性
  - **0083** `change-wt-reset-reliable-size-exact` — Reliable Size 一致必須
  - **0084** `change-wt-error-code-names` — エラーコード名 `WT_*`
  - **0085** `change-wt-draft15-session-semantics` — CLOSE / Origin / Max Streams / 405 ガイダンス

### refs の現状（2026-07-20）

- `refs/draft-ietf-webtrans-http2-15.txt` は追加済み。旧 `refs/draft-ietf-webtrans-http2-14.txt` も参照用に維持する方針（削除しない）
- 本 issue の残作業はソースコメント等の draft 番号機械置換と `CHANGES.md` 追記。意味的な仕様追従は 0081–0085 で行う

順序関係: **0074（コメント同期）→ 0081 → (0082 / 0083) → 0084 → 0085 → 0065/0066** を推奨。

## 変更対象ファイル一覧

### 差し替えるファイル

- `refs/draft-ietf-webtrans-http2-14.txt` を削除し、`update-refs` スキルで取得した最新版 (`refs/draft-ietf-webtrans-http2-XX.txt`) を配置

### 機械的にテキスト置換するファイル

draft 番号テキスト `draft-ietf-webtrans-http2-14` を取得した最新版番号 (`-15` / `-16` 等) に置換するファイルを以下の grep で網羅的に特定する:

```
grep -rln "draft-ietf-webtrans-http2-14" \
  --exclude-dir=closed \
  --exclude-dir=refs \
  --exclude="CHANGES.md" \
  /Users/voluntas/shiguredo/http2-rs/
```

予想される対象 (実行時に grep で再確認):

- `src/error.rs` (Webtransport エラーコード暫定値の言及)
- `src/limits.rs` (WT 初期設定の言及)
- `src/settings.rs` (SETTINGS_WT_* の言及)
- `src/webtransport/mod.rs` (モジュール冒頭 doc コメント)
- `src/webtransport/capsule.rs` (Capsule 仕様の言及)
- `src/webtransport/flow_control.rs` (フロー制御仕様の言及)
- `src/webtransport/init.rs` (WebTransport-Init 仕様の言及)
- `crates/tokio-http2/src/webtransport.rs` 他
- `crates/tokio-http2/README.md`
- `tests/test_webtransport/` 配下
- `pbt/tests/prop_webtransport/` 配下
- `examples/wt_server/`
- `README.md` (もし draft-14 への言及があれば)
- `skills/shiguredo-http2/SKILL.md` (取得した最新版が draft-15 と異なる場合のみ)

### 置換しないファイル

- `CHANGES.md` 全体 (過去リリース履歴の歴史的事実は保全)
- `issues/closed/` 配下すべて (過去 issue の歴史的事実は保全)
- `refs/draft-ietf-webtrans-http2-XX.txt` 本文 (ファイル自体を差し替え)
- `issues/0074-*.md` (本 issue 自体に含まれる draft-14 言及は履歴説明のため保全)
- `issues/0065-*.md` / `issues/0066-*.md` 等の他 open issue (行番号付き引用 `draft-ietf-webtrans-http2-14 Section X.Y L###-L###` が draft 差し替え後に間違った行範囲を指す可能性があるため、本 issue では機械置換しない。draft-15 での該当節の行番号確認と引用更新は、それぞれの open issue が着手される際に個別対応する)
- 推奨 grep コマンド: `grep -rln "draft-ietf-webtrans-http2-14" --exclude-dir=closed --exclude-dir=refs --exclude="CHANGES.md" --exclude="0074-*" /Users/voluntas/shiguredo/http2-rs/issues/ /Users/voluntas/shiguredo/http2-rs/src/ /Users/voluntas/shiguredo/http2-rs/crates/ /Users/voluntas/shiguredo/http2-rs/tests/ /Users/voluntas/shiguredo/http2-rs/pbt/ /Users/voluntas/shiguredo/http2-rs/examples/` で対象ファイルを特定し、`issues/0065-*.md` / `issues/0066-*.md` 等の他 open issue は手動で除外する

## 対応手順

1. 作業ブランチ `feature/change-update-refs-draft-15` を作成する (取得した最新版が draft-15 でなければ、本 issue 着手時に Branch 名・本 issue ファイル名を実バージョン番号でリネームする)
2. `update-refs` スキルを実行し、IETF Datatracker から `draft-ietf-webtrans-http2` の最新版バージョンを確認する。最新版が draft-15 でなければ実バージョン番号 (例: draft-16) を使用する
   - 最新版が `draft-ietf-webtrans-http2-14` のままであれば、refs/ とソースコメントは更新しない。`skills/shiguredo-http2/SKILL.md` 側の draft-15 記述が誤りであることを報告し、本 issue をそのまま実装しない
3. **`update-refs` スキルはダウンロード / ファイル操作の前にユーザー承認を要求する**。承認ゲートで一時停止するため、auto-resolve 経由で本 issue を実行する場合はここでユーザー判断を仰ぐ。承認後に `refs/draft-ietf-webtrans-http2-14.txt` を最新版 (`refs/draft-ietf-webtrans-http2-XX.txt`) に差し替える
4. `diff -u refs/draft-ietf-webtrans-http2-14.txt refs/draft-ietf-webtrans-http2-XX.txt` (旧ファイルは git 履歴から取得) で diff を取得し、本 issue の PR description に貼る (後続 issue 起票判断のため)
5. 「変更対象ファイル一覧」セクションの grep で網羅的に対象ファイルを特定する
6. 上記対象ファイルすべての `draft-ietf-webtrans-http2-14` を最新版番号 (`draft-ietf-webtrans-http2-XX`) に **テキストとして** 置換する。意味的な仕様追従は行わない (Capsule Type 値の数値変更、SETTINGS Identifier 変更、新規 MUST 要件への対応コード追加等はすべて本 issue スコープ外)
7. `skills/shiguredo-http2/SKILL.md` の draft 番号表記の扱い:
   - 取得した最新版が draft-15 だった場合: SKILL.md は既に最新版と一致しているため修正不要 (本 issue の不一致解消は refs/ とソースコメント側を draft-15 に揃えることで達成される)
   - 取得した最新版が draft-15 でない場合 (例: draft-16): SKILL.md の `draft-15` 表記 (15, 170, 222, 414, 487, 496 行等) を最新版番号に揃える
8. `CHANGES.md` の `## develop` セクション内の `[UPDATE]` 群末尾に以下のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネストする:

   ```markdown
   - [UPDATE] WebTransport over HTTP/2 の参照 draft を draft-14 から最新版 (draft-XX) に更新し、ソースコメントの draft 番号表記を一斉に書き換える (issue 0074)
     - @voluntas
   ```

9. PR description に「draft-14 → draft-XX の diff」と「後続起票候補リスト (Capsule Type 値の wire 変更があれば別 issue、SETTINGS Identifier 変更があれば別 issue 等)」を記載する
10. `cargo fmt --all -- --check` で整形違反がないことを確認する
11. `cargo build --workspace` でビルドが成功することを確認する (テキスト置換のみのためビルドエラーが起きない想定)
12. `cargo test --workspace` で全テスト通過を確認する
13. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
14. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `refs/draft-ietf-webtrans-http2-14.txt` が削除され、最新版 `refs/draft-ietf-webtrans-http2-XX.txt` が配置されている
- スキルファイル `skills/shiguredo-http2/SKILL.md` の draft 番号表記が最新版と一致している
- ソースコード・テスト・examples・README・`Cargo.toml` 等 (「変更対象ファイル一覧」の grep で特定されたすべて) で `draft-ietf-webtrans-http2-14` が最新版番号に置換されている
- `CHANGES.md` の過去履歴 / `issues/closed/` の過去 issue / refs/ 内のファイル本文は置換されていない
- `CHANGES.md` の `## develop` に `[UPDATE]` エントリと担当者行が追加されている
- PR description に draft-14 → 最新版の diff と後続起票候補リストが記載されている
- `cargo fmt --all -- --check` が通過する
- `cargo build --workspace` が成功する
- `cargo test --workspace` が成功する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `~/.claude/skills/update-refs/SKILL.md` — refs/ 配下を IETF Datatracker から最新版に更新するスキル
- `refs/draft-ietf-webtrans-http2-14.txt` — 差し替え対象
- `skills/shiguredo-http2/SKILL.md:15,170,222,414,487,496` — draft-15 を参照しているスキル文書
- `src/webtransport/mod.rs:1` — モジュール冒頭 doc コメント (draft-14 言及)
- `src/webtransport/capsule.rs:1` — Capsule 仕様の draft-14 言及
- `src/webtransport/flow_control.rs:5` — フロー制御の draft-14 言及
- `src/error.rs:45-67` — WebTransport エラーコード暫定値の draft-14 言及
- `src/settings.rs:6-100` — SETTINGS_WT_* 暫定値の draft-14 言及
- `src/limits.rs:228-330` — WT 初期設定の draft-14 言及
- `issues/0073-change-rfc9297-non-minimal-varint.md` の `参照` セクション — draft-14 言及が本 issue でも更新対象
- `issues/closed/0021-fix-draft-notes-webtransport.md` — 過去の draft 注記修正の先行事例 (参考)
