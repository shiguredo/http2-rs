# WtStream::send_max() メソッドを削除する

- Priority: Medium
- Created: 2026-08-09
- Completed: {YYYY-MM-DD}
- Branch: feature/change-remove-wtstream-send-max
- Polished: {YYYY-MM-DD}

## 目的

`src/webtransport/stream.rs` の `WtStream::send_max()` getter は全コードベースで一度も呼ばれていない。未使用の公開 API を削除して、利用者が「使うべき API」と誤認するリスクを排除する (issue 0072 の未使用公開 API 削除と同種の対応)。

## 現状

`src/webtransport/stream.rs` の `WtStream` に `send_max()` getter が定義されている:

```rust
/// 送信上限 (ピアが許可した最大バイト数) を取得する
#[must_use]
pub const fn send_max(&self) -> u64 {
    self.send_max
}
```

`src/webtransport.rs` の `WtSession` は送信上限を `WtStream::update_send_max()` 経由でのみ更新・参照しており、`send_max()` getter の呼び出しは存在しない。`send_max` フィールド自体は `send_available()` / `send_data()` / `update_send_max()` の内部で使用されており、フィールドと `update_send_max()` は維持する。兄弟 getter の `recv_max()` は `src/webtransport.rs` の `grow_stream_recv_window` 等で使用されているため維持する。

## 設計方針

- `WtStream::send_max()` getter のみを doc コメントと `#[must_use]` 属性ごと削除する
- `send_max` フィールド・`WtStream::new()` の引数・`update_send_max()` は維持する
- 公開 API 削除のため `[CHANGE]` エントリを `CHANGES.md` の `## develop` に追加する
- `skills/shiguredo-http2/SKILL.md` には `WtStream::send_max()` への直接の言及はない (`WtSession::client` / `WtSession::server` の説明に `send_max` とあるのはフィールド初期化の説明であり、getter の言及ではない)。そのため SKILL.md の変更は不要

## 変更対象ファイル一覧

- `src/webtransport/stream.rs` — `WtStream::send_max()` getter 削除
- `CHANGES.md` — `[CHANGE]` エントリ追加

## 対応手順

1. 作業ブランチ `feature/change-remove-wtstream-send-max` を作成する
2. `src/webtransport/stream.rs` の `WtStream::send_max()` getter を doc コメントごと削除する
3. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の末尾に以下のエントリを追加する:

   ```markdown
   - [CHANGE] `WtStream::send_max()` を削除する (未使用の公開 API。送信上限の更新は `WtStream::update_send_max()` を使用する)
     - @voluntas
   ```

5. `cargo fmt --all -- --check` で整形違反がないことを確認する
6. `cargo test --workspace` で全テスト通過を確認する
7. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
8. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `src/webtransport/stream.rs` から `WtStream::send_max()` getter が削除されている
- `send_max` フィールド・`WtStream::new()` の引数・`update_send_max()` が維持されている
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `src/webtransport/stream.rs` — `WtStream::send_max()` getter
- `issues/closed/0072-change-remove-unused-code.md` — 同種の未使用公開 API 削除の先行事例
