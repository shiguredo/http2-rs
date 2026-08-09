# SendError 型を削除する

- Priority: Medium
- Created: 2026-06-11
- Completed: 2026-08-09
- Polished: 2026-08-08
- Model: deepseek-v4-pro
- Branch: feature/change-remove-send-error

## 目的

`shiguredo_http2::SendError` (`src/send_error.rs`) は issue 0029 で計画され、issues 0024-0032 の Phase 1 (`3ec6793`「構築時検査リファクタリング (issues 0024-0032) の Phase 1: 新型のみ追加する」) で型定義のみ追加されたが、「`Connection::send_*` への統合は別 issue として分離が妥当」とされ、その統合 issue が起票されないまま製品コードでは未使用のまま残っている。`SendError` は canary.3 (2026-05-30) 以降のリリースに含まれる公開 API だが、一度も `Connection` の送信 API に組み込まれていない。本 issue は `SendError` 型一式を削除して未統合の公開 API を整理する。

## 優先度根拠

- `SendError` は公開済みの canary リリース (canary.3〜canary.9) に含まれる公開 API であり、削除は破壊的変更になる。未統合の型を公開 API に残したまま正式リリースすると、将来の互換性負債になるため、正式リリース前のこのタイミングで削除する (0019 の公開 API 削除と同じ扱い)
- 未統合の型を公開 API に残すと、利用者が「使うべき型」と誤認するリスクがある (現状の `Connection::send_*` は `Error` 型を返す)
- 修正コストは低い (ファイル削除 + lib.rs から 2 行削除 + テストファイル削除 + CHANGES.md の `[CHANGE]` エントリ追加 + 既存 `[ADD]` エントリ編集 + SKILL.md の説明行削除)

## 現状の問題

`src/send_error.rs` の module doc コメント (`//!` プレフィックス) で「未統合」と明示されている:

```rust
//! 現時点では `Connection` の送信 API は従来の `Error` 型を使用しており、
//! この型は未統合。統合は送信 API のリファクタリング時に行う。
```

`src/lib.rs` の `pub mod send_error;` にモジュール宣言:

```rust
pub mod send_error;
```

`src/lib.rs` の `pub use send_error::SendError;` に再エクスポート:

```rust
pub use send_error::SendError;
```

`tests/test_send_error.rs` は実在し、`SendError::Display` の出力 5 ケースを検証するテスト 5 件のみを含む。これらは `SendError` 自体の振る舞いを検証するもので、削除しても他型 (`Error` / `FrameError` 等) の振る舞いテストは影響を受けない。

`Connection` の送信系 API (`send_data`, `send_response`, `reset_stream` 等) は全て `Error` 型を返しており、`SendError` は製品コード・`crates/*` / `pbt/` / `fuzz/` / `examples/` のどこからも使われていない。grep 確認結果: `crates/tokio-http2/` / `crates/tokio-nghttp2/` / `crates/shiguredo_nghttp2/` / `pbt/` / `fuzz/` / `examples/` のいずれにも `shiguredo_http2::SendError` の import / 参照は存在しない (参照は `src/send_error.rs` の定義・`src/lib.rs` の再エクスポート・`tests/test_send_error.rs`・`skills/shiguredo-http2/SKILL.md` の説明行のみ)。

## CHANGES.md の扱い

`SendError` は canary.3〜canary.9 の公開済みリリースに含まれる公開 API のため、削除は `[CHANGE]` エントリとして `## develop` セクションに記載する (0019 の公開 API 削除と同じ扱い)。

- `## develop` セクションの既存 `[CHANGE]` 群の末尾に以下のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネストする:

   ```markdown
   - [CHANGE] 未統合の公開 API `SendError` を削除する (`Connection::send_*` は従来どおり `Error` 型を返す)
     - @voluntas
   ```

- 併せて、既存 `[ADD]` エントリの括弧内列挙から `SendError` を除去する (`HeaderFieldError`, `FrameError`, `StreamIdError`, `SettingError`, `LimitsError`, `DecodeError` の 6 種に縮める)。`[CHANGE]` エントリ追加と `[ADD]` エントリからの除去を両方行うのは、公開 API の削除を利用者に告知する一方で、追加時に `SendError` を含めた `[ADD]` エントリの記録を最終差分上は `SendError` なしの形に揃えるため。ただし `(issues 0024-0032)` のサフィックスは既存表記の踏襲として変更しない (規約違反の解消は別途対応とする)

## ブランチ命名 / カテゴリの判断

`SendError` は `pub use` で再エクスポートされた公開 API であり、canary.3〜canary.9 の公開済みリリースに含まれる。公開 API の削除は破壊的変更のため、0019 (`feature/change-remove-dead-code` / `[CHANGE]` 区分) と同じ扱いで `feature/change-` 接頭辞 + `change` カテゴリを採用する。

## 他 issue との関係

本 issue は `src/send_error.rs` / `src/lib.rs` のモジュール宣言と再エクスポート / `tests/test_send_error.rs` / `CHANGES.md` / `skills/shiguredo-http2/SKILL.md` のみを変更し、`src/error.rs` / `src/webtransport/error.rs` / `crates/*` には触れない。機能依存はどの open issue とも無いが、`CHANGES.md` を編集する開いた issue とはマージ順序によって同名ファイルのコンフリクトが発生しうる (内容は異なる箇所なので 3-way merge で解決できる見込み)。

- 0068 (`bug-fix-wt-error-design`) — `WtError` の Display/Debug 修正。`CHANGES.md` に `[FIX]` エントリを追加するため、コンフリクトの可能性がある
- 0070 (`change-privatize-error-wt-error-fields`) — `Error` / `WtError` のフィールド private 化。`skills/shiguredo-http2/SKILL.md` と `CHANGES.md` を編集するため、コンフリクトの可能性がある
- 0072 (`refactor-remove-unused-code`) — `WtError` の未使用ヘルパー削除。`CHANGES.md` を編集するため、コンフリクトの可能性がある
- 0073 — `varint.rs` の修正で `CHANGES.md` 編集不要。無関係
- 0076 (`fmt-translate-english-comments`) — 英語コメント翻訳。`CHANGES.md` の `### misc` にエントリを追加するため、コンフリクトの可能性がある
- 0078 / 0102 / 0103 — `shiguredo_nghttp2` / `src/connection.rs` 系の修正。`CHANGES.md` を編集するため、コンフリクトの可能性がある

## 変更対象ファイル一覧

### 削除するファイル

- `src/send_error.rs` — `SendError` 型定義、`Display` 実装、`std::error::Error` 実装
- `tests/test_send_error.rs` — `SendError::Display` 5 ケースのテスト

### 編集するファイル

- `src/lib.rs` — `pub mod send_error;` を削除
- `src/lib.rs` — `pub use send_error::SendError;` を削除
- `CHANGES.md` — 既存 `[ADD]` エントリの括弧内列挙から `SendError` を除去し、`SendError` 削除の `[CHANGE]` エントリを追加
- `skills/shiguredo-http2/SKILL.md` — `SendError` の説明行 (`- \`SendError\`: 送信側 API 用のエラー型 (現状 \`Connection::send_*\` には未統合で \`Error\` を返す)`) を削除

## 対応手順

1. 作業ブランチ `feature/change-remove-send-error` を作成する
2. `src/send_error.rs` を削除する
3. `tests/test_send_error.rs` を削除する
4. `src/lib.rs` から `pub mod send_error;` と `pub use send_error::SendError;` を削除する。両方を削除しないと、モジュール宣言が残った場合はファイル不在エラー (`E0583: file not found for module 'send_error'`)、再エクスポートが残った場合は未解決 import エラー (`E0433`) でビルドが失敗する
5. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の末尾に `SendError` 削除の `[CHANGE]` エントリを追加し、既存 `[ADD]` エントリの括弧内列挙から `SendError` を除去する (`HeaderFieldError`, `FrameError`, `StreamIdError`, `SettingError`, `LimitsError`, `DecodeError` の 6 種に縮める)
6. `skills/shiguredo-http2/SKILL.md` の `SendError` 説明行を削除する
7. `cargo fmt --all -- --check` で整形違反がないことを確認する
8. `cargo build --workspace` でビルドが成功することを確認する
9. `cargo test --workspace` で全テスト通過を確認する
10. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
11. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する (`SendError` を import していないことの保証)

## 完了条件

- `src/send_error.rs` が削除されている
- `tests/test_send_error.rs` が削除されている
- `src/lib.rs` から `pub mod send_error;` と `pub use send_error::SendError;` が削除されている
- `CHANGES.md` の `## develop` に `SendError` 削除の `[CHANGE]` エントリが追加され、既存 `[ADD]` エントリから `SendError` の言及が除去されている
- `skills/shiguredo-http2/SKILL.md` の `SendError` 説明行が削除されている
- `cargo fmt --all -- --check` が通過する
- `cargo build --workspace` が成功する
- `cargo test --workspace` が成功する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `src/send_error.rs` — 削除対象ファイル
- `src/lib.rs` — `pub mod send_error;` / `pub use send_error::SendError;` 削除対象
- `tests/test_send_error.rs` — 削除対象テストファイル
- `CHANGES.md` — 既存 `[ADD]` エントリの編集対象
- `skills/shiguredo-http2/SKILL.md` — `SendError` 説明行の削除対象
- `issues/closed/0027-change-frame-construct-time-validation.md` — `SendError` 型定義の追加と「Connection::send_* への統合は別 issue 化」の判断元
- `issues/closed/0029-change-split-error-types.md` — エラー型分割の経緯
- `issues/closed/0019-chore-remove-dead-code.md` — 過去の公開 API 削除事例 (`pub use` された API の削除は `[CHANGE]` 区分)

## 解決方法

### `SendError` 型一式の削除

`src/send_error.rs`（型定義・`Display` 実装・`std::error::Error` 実装）と `tests/test_send_error.rs`（`Display` 出力 5 ケースのテスト）を削除し、`src/lib.rs` から `pub mod send_error;` と `pub use send_error::SendError;` を削除した。`skills/shiguredo-http2/SKILL.md` の `SendError` 説明行も削除した。

`Connection::send_*` 系 API は従来どおり `Error` 型を返しており、`SendError` は製品コード・`crates/*` / `pbt/` / `fuzz/` / `examples/` のどこからも参照されていないことを grep で確認済み。

なお、本 issue の本文では公開済みリリースを canary.3〜canary.9 と記載しているが、実際には canary.10 の `src/lib.rs` にも `send_error` が含まれていた。`CHANGES.md` の `[CHANGE]` エントリは公開 API 削除の告知として十分であり、表記ズレは本 issue の記録上のもの。

### CHANGES.md の更新

`[CHANGE]` 群の末尾に `SendError` 削除のエントリを追加し、既存 `[ADD]` エントリの括弧内列挙から `SendError` を除去して 6 種（`HeaderFieldError` / `FrameError` / `StreamIdError` / `SettingError` / `LimitsError` / `DecodeError`）に縮めた。

### 検証

`cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo check --manifest-path fuzz/Cargo.toml` のすべてが通過することを確認した。
