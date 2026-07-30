# SendError 型を削除する

- Priority: Medium
- Created: 2026-06-11
- Polished: 2026-07-31
- Model: deepseek-v4-pro
- Branch: feature/refactor-remove-send-error

## 目的

`shiguredo_http2::SendError` (`src/send_error.rs`) は issue 0029 で計画され issue 0027 で型定義のみ追加されたが、「`Connection::send_*` への統合は別 issue として分離が妥当」とされ、その統合 issue が起票されないまま製品コードでは未使用のまま残っている。`shiguredo_http2` クレートは未リリースであり、`SendError` は同じ develop サイクル内で追加されたきり一度も `Connection` の送信 API に組み込まれていない。本 issue は `SendError` 型一式を削除して未統合の公開 API を整理する。

## 優先度根拠

- `SendError` は **未リリースの develop ブランチ内でのみ存在** する公開 API であり、削除しても外部利用者は影響を受けない。リリース後に削除する場合は SemVer の major bump 相当の破壊的変更になるため、リリース前のこのタイミングを逃すと将来の互換性負債になる
- 未統合の型を公開 API に残すと、利用者が「使うべき型」と誤認するリスクがある (現状の `Connection::send_*` は `Error` 型を返す)
- 修正コストは低い (ファイル削除 + lib.rs から 2 行削除 + テストファイル削除 + CHANGES.md の既存 `[ADD]` エントリ編集)

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

`tests/test_send_error.rs` は実在し、`SendError::Display` の出力 5 ケース (`ConnectionClosed` / `GoawaySent` / `StreamNotOpen` / `FlowControlExhausted` / `HeaderListTooLarge`) を検証するテスト 5 件のみを含む。これらは `SendError` 自体の振る舞いを検証するもので、削除しても他型 (`Error` / `FrameError` 等) の振る舞いテストは影響を受けない。

`Connection` の送信系 API (`send_data`, `send_response`, `reset_stream` 等) は全て `Error` 型を返しており、`SendError` はコードベース内のどこからも使われていない。grep 確認結果: `crates/tokio-http2/` / `crates/tokio-nghttp2/` / `crates/shiguredo_nghttp2/` / `pbt/` / `fuzz/` / `examples/` のいずれにも `shiguredo_http2::SendError` の import / 参照は存在しない (`tests/test_send_error.rs` のみが唯一の参照点)。

未完成の型を公開 API に置くことは利用者を混乱させ、将来の互換性保証の負債になる。

## CHANGES.md の扱い

`CHANGES.md` の `## develop` セクションにある既存 `[ADD]` エントリ:

```
- [ADD] 構築時検査用の公開エラー型 (`HeaderFieldError`, `FrameError`, `StreamIdError`, `SettingError`, `LimitsError`, `SendError`, `DecodeError`) と補助型 ... を追加する (issues 0024-0032)
```

の括弧内列挙に `SendError` が含まれている。`SendError` は同じ `## develop` サイクル内で追加されているため、`shiguredo-changelog` 規約「変更履歴は派生元ブランチとの最終的な差分のみを記載すること」「開発ブランチ内の中間状態の修正は記載しないこと」に従い、本 issue で削除する場合は:

- 既存 `[ADD]` エントリの括弧内列挙から `SendError` を **除去** する (`HeaderFieldError`, `FrameError`, `StreamIdError`, `SettingError`, `LimitsError`, `DecodeError` の 6 種に縮める)
- 新規 `[CHANGE]` エントリは **追加しない** (develop 内で打ち消し合うため、最終的な差分には反映されない)

これにより、CHANGES.md 上で「追加した」と「削除した」が同時に並ぶ自家撞着を回避する。

## ブランチ命名 / カテゴリの判断

未リリース API の削除は外部観測上「最初から存在しなかった」状態に等しいため、`feature/refactor-` 接頭辞 + `refactor` カテゴリを採用する。0019 (`feature/change-remove-dead-code`、`[CHANGE]` 区分) はリリース済み API の削除だったため `change` を採用しているが、本 issue は未リリース API のため `refactor` で扱う。

## 他 issue との関係

本 issue は `src/send_error.rs` / `src/lib.rs` のモジュール宣言と再エクスポート / `tests/test_send_error.rs` / `CHANGES.md` の既存 `[ADD]` エントリ / `skills/shiguredo-http2/SKILL.md` の `SendError` 説明行のみを変更し、`src/error.rs` / `src/webtransport/error.rs` / `crates/*` には触れない。

- 0068 (`bug-fix-wt-error-design`) — `WtError` の Display/Debug 修正、無関係
- 0069 (`bug-fix-nghttp2-send-set-user-data`) — `shiguredo_nghttp2::Session::send()` 修正、無関係
- 0070 (`change-privatize-error-wt-error-fields`) — `Error` / `WtError` のフィールド private 化、無関係
- 0072 (`refactor-remove-unused-code`) — `WtError` の未使用ヘルパー削除、無関係
- 0073-0076 — それぞれ無関係

順序依存なし。0068-0076 のどれと並列マージしてもコンフリクトは発生しない。

## 変更対象ファイル一覧

### 削除するファイル

- `src/send_error.rs` — `SendError` 型定義、`Display` 実装、`std::error::Error` 実装
- `tests/test_send_error.rs` — `SendError::Display` 5 ケースのテスト

### 編集するファイル

- `src/lib.rs` — `pub mod send_error;` を削除
- `src/lib.rs` — `pub use send_error::SendError;` を削除
- `CHANGES.md` — 既存 `[ADD]` エントリの括弧内列挙から `SendError` を除去
- `skills/shiguredo-http2/SKILL.md` — `SendError` の説明行 (`- \`SendError\`: 送信側 API のエラー...`) を削除

## 対応手順

1. 作業ブランチ `feature/refactor-remove-send-error` を作成する
2. `src/send_error.rs` を削除する
3. `tests/test_send_error.rs` を削除する
4. `src/lib.rs` から `pub mod send_error;` と `pub use send_error::SendError;` を削除する。先にモジュール宣言を削除すると後続の行番号がシフトするため、エディタの行番号ジャンプではなく文字列で完全一致削除する。両方を削除しないと、ファイル不在エラー (`E0583: file not found for module 'send_error'`) でビルドが失敗する
5. `CHANGES.md` の `## develop` セクションにある既存 `[ADD]` エントリの括弧内列挙から `SendError` を除去する (`HeaderFieldError`, `FrameError`, `StreamIdError`, `SettingError`, `LimitsError`, `DecodeError` の 6 種に縮める)。新規 `[CHANGE]` エントリは追加しない (develop 内で打ち消し合うため)
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
- `CHANGES.md` の既存 `[ADD]` エントリから `SendError` の言及が除去されている (新規 `[CHANGE]` エントリは追加しない)
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
- `issues/closed/0019-chore-remove-dead-code.md` — 過去の未使用コード削除事例 (リリース済み API 削除のため `[CHANGE]` 区分)
