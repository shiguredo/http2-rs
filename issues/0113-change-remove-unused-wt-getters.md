# WtFlowControl の未使用 getter を削除する

- Priority: Medium
- Created: 2026-08-15
- Completed: {YYYY-MM-DD}
- Branch: feature/change-remove-wt-getters
- Polished: 2026-08-15

## 目的

`src/webtransport/flow_control.rs` の `WtFlowControl` に定義された以下の公開 getter は、全コードベース (src/ / crates/ / tests/ / pbt/ / fuzz/ / examples/) で一度も呼ばれていない。未使用の公開 API を削除して、利用者が「使うべき API」と誤認するリスクを排除する (issue 0072 / 0104 の未使用公開 API 削除と同種の対応):

- `WtFlowControl::max_streams_bidi_remote()`
- `WtFlowControl::max_streams_uni_remote()`
- `WtFlowControl::opened_streams_bidi()`
- `WtFlowControl::opened_streams_uni()`

## 設計方針

- 上記 4 つの getter のみを doc コメントと `#[must_use]` 属性ごと削除する
- `max_streams_bidi_remote` / `max_streams_uni_remote` / `opened_streams_bidi` / `opened_streams_uni` フィールド自体は、`can_open_bidi_stream()` / `can_open_uni_stream()` / `opened_stream()` / `update_max_streams()` / `is_bidi_streams_blocked()` / `is_uni_streams_blocked()` の内部で使用されており維持する
- 公開 API 削除のため `[CHANGE]` エントリを `CHANGES.md` の `## develop` に追加する
- `skills/shiguredo-http2/SKILL.md` には上記 getter への言及はないため変更は不要
- テストでのみ使用される公開 API (例: `WtFlowControl::is_bidi_streams_blocked()` / `is_uni_streams_blocked()`、`WtStream::id()`) は、テストが動作を保証しているため本 issue の削除対象としない

## 変更対象ファイル一覧

- `src/webtransport/flow_control.rs` — 未使用 getter 4 件削除
- `CHANGES.md` — `[CHANGE]` エントリ追加

## 対応手順

1. 作業ブランチ `feature/change-remove-wt-getters` を作成する
2. `src/webtransport/flow_control.rs` の `WtFlowControl::max_streams_bidi_remote()` / `max_streams_uni_remote()` / `opened_streams_bidi()` / `opened_streams_uni()` getter を doc コメントと `#[must_use]` 属性ごと削除する
3. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の末尾に以下のエントリを追加する:

   ```markdown
   - [CHANGE] 未使用の公開 getter `WtFlowControl::max_streams_bidi_remote()` / `max_streams_uni_remote()` / `opened_streams_bidi()` / `opened_streams_uni()` を削除する
     - @voluntas
   ```

4. `cargo fmt --all -- --check` で整形違反がないことを確認する
5. `cargo test --workspace` で全テスト通過を確認する
6. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
7. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `src/webtransport/flow_control.rs` から `WtFlowControl::max_streams_bidi_remote()` / `max_streams_uni_remote()` / `opened_streams_bidi()` / `opened_streams_uni()` getter が削除されている
- 対応するフィールド (max_streams_bidi_remote / max_streams_uni_remote / opened_streams_bidi / opened_streams_uni) と、フィールドを使用するメソッド (`can_open_bidi_stream()` / `can_open_uni_stream()` / `opened_stream()` / `update_max_streams()` / `is_bidi_streams_blocked()` / `is_uni_streams_blocked()`) が維持されている
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `src/webtransport/flow_control.rs` — 削除対象 getter 4 件
- `issues/closed/0072-change-remove-unused-code.md` — 同種の未使用公開 API 削除の先行事例
- `issues/closed/0104-change-remove-send-max-getters.md` — 同種の未使用公開 API 削除の先行事例
