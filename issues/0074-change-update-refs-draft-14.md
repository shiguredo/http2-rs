# refs/ の WebTransport draft を最新版に更新し、ソースコメントの draft 番号を同期する

- Priority: Medium
- Created: 2026-06-11
- Polished: 2026-06-16
- Model: deepseek-v4-pro
- Branch: feature/change-update-refs-draft-14

注: 本 issue の実体はテキスト機械置換と refs/ 差し替えであり、意味的な動作変更はない。本来は `feature/update-` プレフィックスが実態に近いが、shiguredo-issues の慣行 (`change-` がリソース更新を含む) に従い `change-` を維持する。`update-refs` スキルは本 issue の前提として使用するが、SKILL.md / ソースコメント / 他 open issue の機械置換は `update-refs` の責務範囲外であり、本 issue がカバーする拡張作業。本 issue は `update-refs` スキル経由でユーザー承認を要求するため auto-resolve 対象外として運用する。

## 目的

`skills/shiguredo-http2/SKILL.md` が draft-15 を参照している一方、`refs/` には draft-14 が存在し、全ソースコードコメントも draft-14 を参照しているバージョン不一致を解消する。

本 issue は **refs/ ファイルの差し替えと、ソースコメント / SKILL.md 等の draft 番号テキストを最新版に機械的に置換する** ことまでをスコープとする。**仕様変更に伴う実コード修正 (Capsule Type 値の wire 変更、SETTINGS Identifier 変更、新規 MUST 要件の追加対応等) は本 issue では行わず、`update-refs` 実行後に diff を確認したうえで個別の `[CHANGE]` issue として分解起票する**。

## 優先度根拠

- `skills/shiguredo-http2/SKILL.md` (15 行目、170, 222, 414, 487, 496 等) が `draft-ietf-webtrans-http2-15` を参照しているが、実装の根拠は `refs/draft-ietf-webtrans-http2-14.txt`。スキルとリポジトリ実態の不一致は LLM ベースのレビューや他 issue の polish 工程で誤情報を伝播する経路となる
- draft-14 → 最新 (draft-15 以降) で wire レベルの仕様変更 (Capsule Type 値、SETTINGS Identifier、エラーコード、フロー制御要件) があった場合、未追従のままリリースすると相互運用性問題を抱える。本 issue で diff を取得することで、後続 issue 群の起票根拠を確立する
- 修正コスト自体は低い (`update-refs` でファイル差し替え、テキスト一括置換、検証実行)。仕様追従のコストは本 issue のスコープ外として後続 issue で扱う

## 現状の問題

- `refs/draft-ietf-webtrans-http2-14.txt` が現状のリポジトリ内 draft 本文
- `skills/shiguredo-http2/SKILL.md:15` 他は `draft-ietf-webtrans-http2-15` を参照
- ソースコード・テスト・examples・README 等で `draft-ietf-webtrans-http2-14` への言及が数十〜百箇所存在する。正確な対象は作業時に以下の grep で再確認する
  ```
  grep -rln "draft-ietf-webtrans-http2-14" \
    --exclude-dir=closed \
    --exclude-dir=refs \
    --exclude-dir=target \
    --exclude-dir=.git \
    --exclude="CHANGES.md" \
    --exclude="0074-*" \
    .
  ```
- 本 issue 作成時点の IETF Datatracker では `draft-ietf-webtrans-http2` の最新版は `draft-ietf-webtrans-http2-14` であった。`draft-ietf-webtrans-http2-15` は公式に公開されていなかったが、`skills/shiguredo-http2/SKILL.md` は `draft-ietf-webtrans-http2-15` を参照している。そのため、本 issue の作業時に `update-refs` スキルで最新版を再確認し、最新版が draft-14 のままであれば refs/ とソースコメントを draft-15 へ更新してはならない

バージョン不一致により:

- スキル経由のレビューや指示が「最新 draft に対応」と告げるが実装は draft-14 のまま
- draft-15 以降で仕様が変更された場合、未追従の問題箇所を特定する手段がない (diff が手元にないため)
- `WtErrorCode::WebtransportError = 0x100` 等の暫定値、`Capsule::WT_*` の暫定 Capsule Type 値、`SETTINGS_WT_*` の暫定 SETTINGS Identifier (0x2b61〜0x2b66) が draft-15 以降で IANA 登録または値変更されている可能性がある

## 設計方針

### 3 つの分岐ケース

`update-refs` スキルを実行し、IETF Datatracker (`https://datatracker.ietf.org/api/v1/doc/document/?name=draft-ietf-webtrans-http2&format=json`) の `rev` フィールドから最新版バージョンを取得する。取得した最新版に応じて 3 ケースに分岐する:

| ケース | 最新版 | refs/ 差し替え | ソースコメント / 他 open issue 置換 | SKILL.md 修正 |
|--------|--------|---------------|----------------------------------|---------------|
| A | draft-14 のまま | しない | しない | `draft-ietf-webtrans-http2-15` → `-14` に機械置換 (SKILL.md 側の記述ミスを是正) |
| B | draft-15 | 差し替え (draft-14 → draft-15) | `-14` → `-15` に機械置換 | 既に `-15` と一致のため不要 |
| C | draft-16 以降 | 差し替え (draft-14 → 最新版) | `-14` → 最新版番号に機械置換 | `-15` → 最新版番号に機械置換 |

### 共通方針

- 意味的な仕様追従は本 issue では行わない (Capsule Type 値の wire 変更、SETTINGS Identifier 変更、新規 MUST/SHOULD 要件への対応コード追加等)
- ケース B / C で refs/ を差し替えた場合、draft-14 と最新版の diff を取得して PR description に記載する。GitHub PR description の 65536 文字上限を超える場合は、(a) gist として添付 + URL を PR description に記載、(b) 別 commit として `docs/draft-diff.txt` を追加、のいずれかを採用する
- 後続起票候補リスト (Capsule Type 値変更、SETTINGS Identifier 変更、エラーコード値変更等) は PR description に記載し、本 issue マージ後に `create-issue` スキルで各起票候補を個別 issue として作成する。PR description の記載だけだと将来 PR 履歴から失われやすいため、`docs/draft-update-followups.md` を追加する選択肢も検討する
- Postel の法則に従い、機械置換は完了条件で `cargo build` / `cargo test` / `cargo clippy` の全通過を保証してから完了とする
- ソースコメント内の行番号付き引用 (`draft-ietf-webtrans-http2-14 Section X.Y L###-L###` 等) は、refs/ 差し替え後に行番号が変わる可能性がある。機械置換後に該当箇所の行番号が最新版 refs と一致しているか目視で確認する

## スコープ外

- **仕様変更に伴う実コード修正**: Capsule Type 値の wire 変更、SETTINGS Identifier 変更、エラーコード値変更、新規 MUST/SHOULD 要件の追加対応、WT_CLOSE_SESSION reason 上限の変更追従、WebTransport-Init ヘッダーフォーマット変更追従等は **本 issue では行わない**。`update-refs` 実行後の diff を確認したうえで個別の `[CHANGE]` issue (例: `change-follow-draft-15-capsule-type-iana`) として分解起票する
- **過去 CHANGES.md エントリ・closed issues・refs 原本本文の置換は禁止**:
  - `CHANGES.md` の `## バージョン` 配下にある過去リリース履歴の draft-14 言及 (歴史的事実) は置換しない
  - `issues/closed/` 配下の過去 issue ファイル内の draft-14 言及 (歴史的事実) は置換しない
  - `refs/draft-ietf-webtrans-http2-14.txt` 本文の `Internet-Draft draft-ietf-webtrans-http2-14` は置換不要 (ファイル自体を最新版に差し替える)
- SKILL.md の draft-15 表記が draft-15 (取得した最新版が draft-15 と一致する場合) であれば修正不要。最新版が draft-16 以降だった場合は SKILL.md 全体を最新版番号に揃える
- `pbt/tests/prop_webtransport/main.rs` 内の Capsule Type 数値 (`0x190B4D3D` 等) の hex 値変更は本 issue スコープ外 (上記の分解起票で扱う)

## 他 issue との関係

- **0068 (`bug-fix-wt-error-display-info-leak`)**: `src/webtransport/error.rs` の draft-14 コメントに本 issue が触れる可能性があり、0068 マージ後にマージするのが安全
- **0070 (`change-privatize-error-wt-error-fields`)**: 同上、`src/webtransport/error.rs` への影響あり。0070 マージ後にマージする
- **0072 (`refactor-remove-unused-code`)**: `src/webtransport/error.rs` / `src/webtransport/flow_control.rs` への影響あり。0072 マージ後にマージする
- **0073 (`change-rfc9297-allow-non-minimal-varint`)**: `src/webtransport/varint.rs` 自体には draft-14 への言及はないが、0073 issue ファイルの `参照` セクションに `refs/draft-ietf-webtrans-http2-14.txt` への言及がある。本 issue で refs/ を差し替える際に、0073 issue ファイルの該当参照も同時に最新版番号に置換する。0073 は既に `Polished: 2026-06-14` であるため、本 issue 内で 0073 ファイルを編集してもよい
- **0075 (`fmt-replace-unwrap-with-expect`) / 0076 (`fmt-translate-english-comments`)**: それぞれ無関係
- **0065 / 0066 (open)**: WebTransport モジュール本体を触る issue で、本 issue マージ後に着手するのが安全

順序関係: **0068 → 0070 → 0072 → 0073 → 0074 → 0065/0066** の順を推奨。

## 変更対象ファイル一覧

### 差し替えるファイル

- `refs/draft-ietf-webtrans-http2-14.txt` を削除し、`update-refs` スキルで取得した最新版 (`refs/draft-ietf-webtrans-http2-XX.txt`) を配置

### 機械的にテキスト置換するファイル

draft 番号テキスト `draft-ietf-webtrans-http2-14` を取得した最新版番号 (`-15` / `-16` 等) に置換するファイルを以下の grep で網羅的に特定する:

```
grep -rln "draft-ietf-webtrans-http2-14" \
  --exclude-dir=closed \
  --exclude-dir=refs \
  --exclude-dir=target \
  --exclude-dir=.git \
  --exclude="CHANGES.md" \
  --exclude="0074-*" \
  .
```

この例では `issues/` 配下の open issue ファイルもヒットするが、後述「置換しないファイル」の通り、open issue ファイル (`issues/0065-*.md` / `issues/0066-*.md` 等) は機械置換対象外とし、手動で除外する。

予想される対象 (実行時に grep で再確認):

- `src/connection/mod.rs` (Extended CONNECT / WebTransport 対応の言及)
- `src/error.rs` (WebTransport エラーコード暫定値の言及)
- `src/limits.rs` (WT 初期設定の言及)
- `src/settings.rs` (SETTINGS_WT_* の言及)
- `src/webtransport/mod.rs` (モジュール冒頭 doc コメント)
- `src/webtransport/capsule.rs` (Capsule 仕様の言及)
- `src/webtransport/error.rs` (WebTransport エラー型の言及)
- `src/webtransport/flow_control.rs` (フロー制御仕様の言及)
- `src/webtransport/init.rs` (WebTransport-Init 仕様の言及)
- `src/webtransport/stream.rs` (WebTransport ストリーム仕様の言及)
- `crates/tokio-http2/README.md` (対応仕様の言及)
- `crates/tokio-http2/src/server.rs` (WebTransport 対応の言及)
- `crates/tokio-http2/src/tls.rs` (TLS 要件の言及)
- `crates/tokio-http2/src/webtransport.rs` (WebTransport サーバー API の言及)
- `crates/tokio-http2/tests/test_webtransport.rs` (WebTransport 統合テストの言及)
- `tests/test_error.rs` (WebTransport エラーコードの言及)
- `tests/test_webtransport/` 配下
- `pbt/tests/prop_error.rs` (WebTransport エラーコードの言及)
- `examples/wt_server/` 配下
- `README.md` (もし draft-14 への言及があれば)
- `skills/shiguredo-http2/SKILL.md` (取得した最新版が draft-15 と異なる場合のみ)

### 置換しないファイル

- `CHANGES.md` 全体 (過去リリース履歴の歴史的事実は保全)
- `issues/closed/` 配下すべて (過去 issue の歴史的事実は保全)
- `refs/draft-ietf-webtrans-http2-XX.txt` 本文 (ファイル自体を差し替え)
- `issues/0074-*.md` (本 issue 自体に含まれる draft-14 言及は履歴説明のため保全)
- `issues/0065-*.md` / `issues/0066-*.md` / `issues/0070-*.md` 等の他 open issue (行番号付き引用 `draft-ietf-webtrans-http2-14 Section X.Y L###-L###` が draft 差し替え後に間違った行範囲を指す可能性があるため、本 issue では機械置換しない。draft-15 での該当節の行番号確認と引用更新は、それぞれの open issue が着手される際に個別対応する)
- ただし `issues/0073-change-rfc9297-non-minimal-varint.md` は例外とし、`参照` セクション (本 issue 確認時点で L18 / L93 / L163 該当) の `refs/draft-ietf-webtrans-http2-14.txt` ファイル名部分を最新版番号に機械置換する。`参照` セクション内に存在する本 issue 名・ブランチ名への言及 (例: `change-update-refs-draft-14`) は置換しない (本 issue 自体の識別子のため)。行番号部分 (L211-L217 等) は最新版 refs で再確認して一致させる (この行番号調整は機械置換ではなく半手動の意味的調整)

## 対応手順

1. 作業ブランチ `feature/change-update-refs-draft-14` を作成する
2. `update-refs` スキルを実行し、IETF Datatracker `rev` フィールドから `draft-ietf-webtrans-http2` の最新版バージョンを確認する。**`update-refs` はダウンロード / ファイル操作の前にユーザー承認を要求する**ため、承認ゲートで一時停止する (auto-resolve 経由では実行しない)
3. 設計方針セクションの分岐表に従って分岐 (A/B/C):
   - **ケース A (最新版が draft-14)**: refs/ とソースコメントは更新せず、`skills/shiguredo-http2/SKILL.md` の `draft-ietf-webtrans-http2-15` を `draft-ietf-webtrans-http2-14` に機械置換する (SKILL.md 側の記述ミスを是正)。手順 4-6 はスキップして手順 7 へ
   - **ケース B (最新版が draft-15)** / **ケース C (draft-16 以降)**: 手順 4 へ進む
4. 旧 `refs/draft-ietf-webtrans-http2-14.txt` の一時コピーを取得した後、`update-refs` で最新版に差し替える。`diff -u <一時コピー> refs/draft-ietf-webtrans-http2-XX.txt` で diff を取得する
5. 「変更対象ファイル一覧」セクションの grep で網羅的に対象ファイルを特定する (grep コマンドは「現状の問題」セクションに掲載済み)
6. 上記対象ファイルすべての `draft-ietf-webtrans-http2-14` を最新版番号 (`draft-ietf-webtrans-http2-XX`) に **テキストとして** 置換する。`issues/0073-change-rfc9297-non-minimal-varint.md` の例外扱い (ファイル名部分のみ置換、行番号再確認) は「置換しないファイル」セクションを参照
7. ケース C のみ: SKILL.md 内の `draft-ietf-webtrans-http2-15` テキストを最新版番号に機械置換する。置換後に `grep -n "draft-ietf-webtrans-http2-15" skills/shiguredo-http2/SKILL.md` で未置換箇所が残っていないか確認する (ケース B では既に `-15` と一致のため修正不要)
8. `CHANGES.md` の `## develop` セクション内の `### misc` 配下にある `[UPDATE]` 群末尾に以下のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネストする (`[UPDATE]` は機能に直接影響しない参照資料更新であるため `### misc` 配下が適切):

   ```markdown
   - [UPDATE] WebTransport over HTTP/2 の参照 draft を draft-14 から最新版 (draft-XX) に更新し、ソースコメントの draft 番号表記を一斉に書き換える
     - @voluntas
   ```

9. PR description に以下を記載 (ケース B / C のみ):
   - 「draft-14 → 最新版の diff」(65536 文字を超える場合は gist 添付 + URL または `docs/draft-diff.txt` として別 commit)
   - 「後続起票候補リスト」(Capsule Type 値の wire 変更、SETTINGS Identifier 変更、エラーコード値変更、新規 MUST 要件等)。本 issue マージ後に `create-issue` スキルで個別 issue として作成する
10. `cargo fmt --all -- --check` で整形違反がないことを確認する
11. `cargo test --workspace` で全テスト通過を確認する (test は内部でビルドも兼ねるため `cargo build` は省略可。意味的な変更はないため通常はビルドエラーは起きないが、念のため検証する)
12. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
13. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

設計方針セクションの分岐表に従いケース A/B/C で完了条件が異なる。共通条件と分岐別条件を分けて記載する。

### 共通完了条件

- `CHANGES.md` の `## develop` の `### misc` 配下に `[UPDATE]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が成功する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する
- `CHANGES.md` の過去履歴 / `issues/closed/` の過去 issue / refs/ 内のファイル本文 / `issues/0074-*.md` (本 issue 自体) は置換されていない

### ケース A 専用 (最新版が draft-14)

- refs/ とソースコメントは差し替え / 置換していない (draft-14 のまま)
- `skills/shiguredo-http2/SKILL.md` 内の `draft-ietf-webtrans-http2-15` がすべて `draft-ietf-webtrans-http2-14` に置換されている (`grep -n "draft-ietf-webtrans-http2-15" skills/shiguredo-http2/SKILL.md` で残ヒット 0)

### ケース B / C 専用 (最新版が draft-15 以降)

- `refs/draft-ietf-webtrans-http2-14.txt` が削除され、最新版 `refs/draft-ietf-webtrans-http2-XX.txt` が配置されている
- ソースコード・テスト・examples・README 等 (「変更対象ファイル一覧」の grep で特定されたすべて) で `draft-ietf-webtrans-http2-14` が最新版番号に置換されている
- `issues/0073-change-rfc9297-non-minimal-varint.md` の `参照` セクションにある `refs/draft-ietf-webtrans-http2-14.txt` ファイル名部分が最新版番号に置換され、行番号が最新版 refs と一致している
- PR description に draft-14 → 最新版の diff (または gist URL / `docs/draft-diff.txt` への参照) と後続起票候補リストが記載されている
- ケース C のみ: `skills/shiguredo-http2/SKILL.md` 内の `draft-ietf-webtrans-http2-15` がすべて最新版番号に置換されている (ケース B では SKILL.md は既に `-15` と一致のため修正不要)

## 参照

- `update-refs` スキル — refs/ 配下を IETF Datatracker から最新版に更新するスキル
- `refs/draft-ietf-webtrans-http2-14.txt` — 差し替え対象
- `skills/shiguredo-http2/SKILL.md` — `draft-ietf-webtrans-http2-15` を参照している箇所 (15, 170, 222, 414, 487, 496 行等) があるスキル文書。行番号は改訳で変わる可能性があるため、作業時にテキストパターン `draft-ietf-webtrans-http2-15` で grep して確認する
- `src/webtransport/mod.rs:1` — モジュール冒頭 doc コメント (draft-14 言及)
- `src/webtransport/capsule.rs:1` — Capsule 仕様の draft-14 言及
- `src/webtransport/flow_control.rs:5` — フロー制御の draft-14 言及
- `src/error.rs:45-67` — WebTransport エラーコード暫定値の draft-14 言及
- `src/settings.rs:6-100` — SETTINGS_WT_* 暫定値の draft-14 言及
- `src/limits.rs:228-330` — WT 初期設定の draft-14 言及
- `issues/0073-change-rfc9297-non-minimal-varint.md` の `参照` セクション — draft-14 言及が本 issue でも更新対象
- `issues/closed/0021-fix-draft-notes-webtransport.md` — 過去の draft 注記修正の先行事例 (参考)
