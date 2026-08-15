# 未使用の send_max() getter を削除する

- Priority: Medium
- Created: 2026-08-09
- Completed: 2026-08-15
- Branch: feature/change-remove-send-max
- Polished: 2026-08-15

## 目的

`src/webtransport/stream.rs` の `WtStream::send_max()` と `src/webtransport/flow_control.rs` の `WtFlowControl::send_max()` は、全コードベース (src/ / crates/ / tests/ / pbt/ / fuzz/ / examples/) で一度も呼ばれていない。未使用の公開 API を削除して、利用者が「使うべき API」と誤認するリスクを排除する (issue 0072 の未使用公開 API 削除と同種の対応)。

## 現状

`WtStream::send_max()` getter は `src/webtransport/stream.rs` に定義されている:

```rust
/// 送信上限 (ピアが許可した最大バイト数) を取得する
#[must_use]
pub const fn send_max(&self) -> u64 {
    self.send_max
}
```

`WtFlowControl::send_max()` getter も同様に `src/webtransport/flow_control.rs` に定義されている:

```rust
/// 送信上限 (ピアが許可した最大バイト数) を取得する
#[must_use]
pub const fn send_max(&self) -> u64 {
    self.send_max
}
```

`src/webtransport.rs` の `WtSession` は送信上限を `WtStream::update_send_max()` / `WtFlowControl::update_send_max()` 経由でのみ更新しており、`send_max()` getter の呼び出しは存在しない。`send_max` フィールド自体は `WtStream` では `send_available()` / `send_data()` / `update_send_max()`、`WtFlowControl` では `send_available()` / `consume_send()` / `update_send_max()` の内部で使用されており、フィールドと `update_send_max()` は維持する。兄弟 getter の `recv_max()` は `WtStream` では `src/webtransport.rs` の `grow_stream_recv_window`、`WtFlowControl` では `grow_recv_window` で使用されているため維持する。

## 設計方針

- `WtStream::send_max()` / `WtFlowControl::send_max()` getter のみを doc コメントと `#[must_use]` 属性ごと削除する
- `send_max` フィールド・`new()` の引数・`update_send_max()` は維持する
- 公開 API 削除のため `[CHANGE]` エントリを `CHANGES.md` の `## develop` に追加する
- `skills/shiguredo-http2/SKILL.md` には `send_max()` getter への直接の言及はない (`WtSession::client` / `WtSession::server` の説明の `send_max` はフィールド初期化の説明であり、getter の言及ではない)。そのため SKILL.md の変更は不要

## 変更対象ファイル一覧

- `src/webtransport/stream.rs` — `WtStream::send_max()` getter 削除
- `src/webtransport/flow_control.rs` — `WtFlowControl::send_max()` getter 削除
- `CHANGES.md` — `[CHANGE]` エントリ追加

## 対応手順

1. 作業ブランチ `feature/change-remove-send-max` を作成する
2. `src/webtransport/stream.rs` の `WtStream::send_max()` getter を doc コメントと `#[must_use]` 属性ごと削除する
3. `src/webtransport/flow_control.rs` の `WtFlowControl::send_max()` getter を doc コメントと `#[must_use]` 属性ごと削除する
4. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の末尾に以下のエントリを追加する:

   ```markdown
   - [CHANGE] 未使用の公開 getter `WtStream::send_max()` / `WtFlowControl::send_max()` を削除する
     - @voluntas
   ```

5. `cargo fmt --all -- --check` で整形違反がないことを確認する
6. `cargo test --workspace` で全テスト通過を確認する
7. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
8. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `src/webtransport/stream.rs` から `WtStream::send_max()` getter が削除されている
- `src/webtransport/flow_control.rs` から `WtFlowControl::send_max()` getter が削除されている
- `send_max` フィールド・`new()` の引数・`update_send_max()` が維持されている
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `src/webtransport/stream.rs` — `WtStream::send_max()` getter
- `src/webtransport/flow_control.rs` — `WtFlowControl::send_max()` getter
- `issues/closed/0072-change-remove-unused-code.md` — 同種の未使用公開 API 削除の先行事例

## 解決方法

`src/webtransport/stream.rs` の `WtStream::send_max()` と `src/webtransport/flow_control.rs` の `WtFlowControl::send_max()` を、doc コメントと `#[must_use]` 属性ごと削除した。いずれも全コードベースで呼び出しが存在しないことを grep で確認済み。`send_max` フィールド・`new()` の引数・`update_send_max()` は維持し、`recv_max()` などの使用中の getter は削除していない。

`CHANGES.md` の `## develop` の既存 `[CHANGE]` 群の末尾にエントリを追加した。テストの追加・変更はない (削除対象の getter を使用するテストは存在しなかった)。

検証として `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo check --manifest-path fuzz/Cargo.toml` のすべてが通過することを確認した。
