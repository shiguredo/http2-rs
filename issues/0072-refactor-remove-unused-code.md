# 未使用コードを一括削除する

- Priority: Medium
- Created: 2026-06-11
- Polished: 2026-06-14
- Model: deepseek-v4-pro
- Branch: feature/refactor-remove-unused-code

## 目的

コードベース内の未使用コード (死にコード・呼び出しのない関数・未使用の `Default` 実装等) を一括削除する。grep で全コードベース (`src/` / `crates/` / `tests/` / `pbt/` / `fuzz/` / `examples/`) を確認した結果、以下の 8 項目が未使用または不要であることが判明している。

## 優先度根拠

- `shiguredo_http2` クレートは未リリースの状態で develop ブランチで開発中。未使用 API を残したままリリースすると、利用者が「使うべき API」と誤認するリスクがある
- 未使用 API は SemVer の major bump 相当の破壊的変更でしか削除できなくなるため、リリース前のこのタイミングを逃すと将来の互換性負債になる
- 修正コストは低い (関数 / 実装の削除と一部テスト書き換え)
- 0068 (`bug-fix-wt-error-display-info-leak`) は本 issue より先にマージされる前提であり、0068 マージ後に `WtErrorKind::SessionClosed` 等を削除する作業を本 issue で担うため、相対的に着手の優先度はある

## 削除対象一覧

### 1. `WtError::incomplete()` ヘルパー

**ファイル**: `src/webtransport/error.rs`
**削除範囲**: ヘルパー関数定義 (`#[track_caller]` 属性行・doc コメントを含む)

```rust
/// 入力データ不足エラーを生成する
#[track_caller]
pub fn incomplete() -> Self {
    Self::new(WtErrorKind::Incomplete)
}
```

**根拠**: 全コードベースで一度も呼ばれない。`src/webtransport/varint.rs` 内では `WtError::new(WtErrorKind::Incomplete)` が直接使われている。`WtErrorKind::Incomplete` 列挙子自体は `varint.rs` (line 144, 158, 166, 177) と `capsule.rs` (line 333, 344) で使用されており **維持** する。

### 2. `WtError::buffer_too_short()` ヘルパー

**ファイル**: `src/webtransport/error.rs`
**削除範囲**: ヘルパー関数定義 (`#[track_caller]` 属性行・doc コメントを含む)

```rust
/// バッファ不足エラーを生成する
#[track_caller]
pub fn buffer_too_short() -> Self {
    Self::new(WtErrorKind::BufferTooShort)
}
```

**根拠**: 全コードベース (src / tests / pbt / fuzz / examples / crates) で一度も呼ばれない。`varint.rs:71` で `WtErrorKind::BufferTooShort` を `WtError::with_reason` で構築している。`WtErrorKind::BufferTooShort` 列挙子自体は維持する。

### 3. `WtError::session_closed()` ヘルパーと `WtErrorKind::SessionClosed` 列挙子

**ファイル**: `src/webtransport/error.rs`
**削除範囲**:
- `WtErrorKind::SessionClosed` 列挙子定義 (`pub enum WtErrorKind` 内)
- `impl std::fmt::Display for WtErrorKind` の対応 arm (`Self::SessionClosed => write!(f, "SessionClosed"),`)
- `WtError::session_closed()` ヘルパー関数定義 (`#[track_caller]` 属性行・doc コメントを含む)

```rust
/// セッションクローズエラーを生成する
#[track_caller]
pub fn session_closed<T: Into<String>>(reason: T) -> Self {
    Self::with_reason(WtErrorKind::SessionClosed, reason)
}
```

**根拠**: セッションクローズは状態遷移 (`WtSessionState::Closed`) で表現されており、エラー経路では `session_closed()` ヘルパーの呼び出しが存在しない。`WtErrorKind::SessionClosed` の参照元も `session_closed()` ヘルパーのみのため、列挙子と Display arm も同時に削除する。

### 4. `stream_id::stream_type()` 関数

**ファイル**: `src/webtransport/stream.rs`
**削除範囲**: 関数定義 (doc コメントを含む)

```rust
/// ストリームタイプを取得
#[must_use]
pub const fn stream_type(id: WtStreamId) -> u8 {
    (id & 0x03) as u8
}
```

**根拠**: 全コードベースで呼び出しなし。兄弟関数 `is_client_initiated()` / `is_bidirectional()` の組み合わせで等価に判定可能であり、`stream_type()` は冗長。`webtransport::stream::stream_id::stream_type` として外部公開されているが未リリースのため利用者影響なし。

### 5. `SendBuffer::clear()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (doc コメントを含む)

**根拠**: 製品コード・テストコード・pbt・fuzz・examples のすべてで呼び出しなし。

### 6. `RecvBuffer::clear()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (doc コメントを含む)

**根拠**: 製品コード・テストコード・pbt・fuzz・examples のすべてで呼び出しなし。

### 7. `RecvBuffer::take()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (doc コメントを含む) と関連テストの修正

**根拠**: 製品コードからの呼び出しはなく、`tests/test_stream/buffer.rs:29,56` (`test_recv_buffer_push_pop` / `test_recv_buffer_push_uses_saturating_add`) でのみ使用されている。`pop(buf.len())` で等価の動作が得られるため、`take()` を削除して当該テストを `pop(buf.len())` 形式に書き換える方針とする。テスト自体は受信バッファの全データ取り出し動作の保証として有用なので削除せず維持する。

### 8. `impl Default for WtFlowControl`

**ファイル**: `src/webtransport/flow_control.rs`
**削除範囲**: `impl Default for WtFlowControl` ブロック全体 (doc コメントは存在しない)

**根拠**: 全コードベースで `WtFlowControl::default()` が一度も呼ばれていない。`WtFlowControl::new(...)` は `WtSession::new()` 内で `WtConfig` の値を引数として明示的に呼ばれており、`WtConfig::default()` 経由のセッション生成時にも `WtFlowControl::default()` を経由しない (`WtConfig::default()` の各フィールド値で `WtFlowControl::new(...)` が呼ばれる)。`Default` 実装の削除で動作変更は発生しない。

## CHANGES.md の扱い

削除対象 8 項目のうち、`WtError` 系 (#1〜#3) と `stream_type` (#4) と `WtFlowControl::Default` (#8) は draft-ietf-webtrans-http2 対応として develop で追加された未リリース API のため、`shiguredo-changelog` 規約「変更履歴は派生元ブランチとの最終的な差分のみを記載すること」「開発ブランチ内の中間状態の修正は記載しないこと」に従い、最終差分には含まれない (CHANGES.md 編集不要)。

ただし `SendBuffer::clear` / `RecvBuffer::clear` / `RecvBuffer::take` (#5〜#7) は HTTP/2 ストリームバッファ機能の一部で過去のリリースに含まれている可能性があるため、`CHANGES.md` の `## develop` セクションに `[CHANGE]` エントリ 1 件を追加する (1 行集約パターン、0019 のスタイルに倣う)。

CHANGES.md エントリ文言案:

```markdown
- [CHANGE] 製品コードで未使用の公開 API (`SendBuffer::clear`, `RecvBuffer::clear`, `RecvBuffer::take`) を削除する。`RecvBuffer::take` を使用していたテストは `pop(buf.len())` 形式に書き換える
  - @voluntas
```

## 他 issue との関係

- **0068 (`bug-fix-wt-error-display-info-leak`)**: 本 issue は 0068 マージ後にマージされる前提。0068 のテストでは `WtErrorKind::SessionClosed` / `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` を使用しないことが既に明記されている。0068 マージ後に `WtErrorKind::SessionClosed` の参照元は本 issue で削除される `WtError::session_closed()` のみとなる
- **0069 (`bug-fix-nghttp2-send-set-user-data`)**: `shiguredo_nghttp2` 内の修正で、`shiguredo_http2` の `WtError` 等とは無関係。順序依存なし
- **0070 (`change-privatize-error-wt-error-fields`)**: 0068 マージ後であれば、0070 → 0072 / 0072 → 0070 のどちらの順序でマージしても衝突しない。0070 のテストでも本 issue で削除予定の API は使用しない
- **0071 (`refactor-remove-send-error`)**: `SendError` 削除の独立 issue。コンフリクトなし
- **0077 (`change-tokio-http2-error-add-webtransport-variant`)**: 0072 で `WtErrorKind::SessionClosed` を削除するため、0077 の単体テストでは削除されない `WtErrorKind::Incomplete` を使用するよう調整済み。0072 → 0077 の順序でマージしてもコンパイルエラーにならない
- **0073 (`change-rfc9297-non-minimal-varint`)**: `varint.rs` を改修する可能性があり、本 issue が削除する `WtError::incomplete` ヘルパーは `varint.rs` で使用されていないため独立。`WtErrorKind::Incomplete` 列挙子は本 issue で維持する
- **0074-0076**: それぞれ無関係

順序依存は 0068 → 0072 のみ。

## 単一カテゴリ性について

8 項目はそれぞれ異なるサブ領域 (WtError 系 / WebTransport ストリーム ID / HTTP/2 ストリームバッファ / WebTransport フロー制御) にまたがるが、「未使用の公開 API を削除する」という単一の論理目的に集約している。先行事例 0019 (`chore-remove-dead-code`) も複数領域 (frame encoder / decoder / stream_state) をまたいで「未使用コード一括削除」として 1 issue で扱った前例があり、本 issue もこれに倣う。

## 変更対象ファイル一覧

### 編集するファイル

- `src/webtransport/error.rs` — `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` / `WtErrorKind::SessionClosed` 削除、`Display` arm 削除
- `src/webtransport/stream.rs` — `stream_id::stream_type` 関数削除
- `src/stream/buffer.rs` — `SendBuffer::clear` / `RecvBuffer::clear` / `RecvBuffer::take` 削除
- `src/webtransport/flow_control.rs` — `impl Default for WtFlowControl` 削除
- `tests/test_stream/buffer.rs` — `test_recv_buffer_push_pop` 内の `buf.take()` を `pop(buf.len())` 形式に、`test_recv_buffer_push_uses_saturating_add` 内の `buf2.take()` を `pop(buf2.len())` 形式に書き換え
- `CHANGES.md` — `[CHANGE]` エントリ 1 件追加

## 対応手順

1. 作業ブランチ `feature/refactor-remove-unused-code` を作成する
2. `src/webtransport/error.rs` の `WtError::incomplete()` / `WtError::buffer_too_short()` / `WtError::session_closed()` を `#[track_caller]` 属性行と doc コメントごと削除する
3. `src/webtransport/error.rs` の `WtErrorKind::SessionClosed` 列挙子と、`impl std::fmt::Display for WtErrorKind` の `Self::SessionClosed => write!(f, "SessionClosed"),` arm を削除する
4. `src/webtransport/stream.rs` の `stream_id::stream_type()` 関数を doc コメントごと削除する
5. `src/stream/buffer.rs` の `SendBuffer::clear` / `RecvBuffer::clear` / `RecvBuffer::take` メソッドを doc コメントごと削除する
6. `tests/test_stream/buffer.rs` の `test_recv_buffer_push_pop` 内の `buf.take()` を `pop(buf.len())` 形式に、`test_recv_buffer_push_uses_saturating_add` 内の `buf2.take()` を `pop(buf2.len())` 形式に書き換える
7. `src/webtransport/flow_control.rs` の `impl Default for WtFlowControl` ブロックを削除する
8. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の末尾に上記の `[CHANGE]` エントリと担当者行を追加する
9. `cargo fmt --all -- --check` で整形違反がないことを確認する
10. `cargo build --workspace` でビルドが成功することを確認する
11. `cargo test --workspace` で全テスト通過を確認する
12. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
13. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `src/webtransport/error.rs` から `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` / `WtErrorKind::SessionClosed` (および対応する `Display` arm) が削除されている
- `src/webtransport/stream.rs` から `stream_id::stream_type` 関数が削除されている
- `src/stream/buffer.rs` から `SendBuffer::clear` / `RecvBuffer::clear` / `RecvBuffer::take` が削除されている
- `tests/test_stream/buffer.rs` の `buf.take()` / `buf2.take()` 呼び出しが、それぞれ `pop(buf.len())` / `pop(buf2.len())` 形式に書き換えられている
- `src/webtransport/flow_control.rs` から `impl Default for WtFlowControl` が削除されている
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリ 1 件と担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo build --workspace` が成功する
- `cargo test --workspace` が成功する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `issues/closed/0019-chore-remove-dead-code.md` — 過去の未使用コード一括削除の先行事例
- `issues/0068-bug-fix-wt-error-display-info-leak.md` — 0068 が本 issue より先にマージされる前提であり、`WtErrorKind::SessionClosed` 等の削除を本 issue で担う
- `src/webtransport/error.rs` — `WtError` / `WtErrorKind` の未使用ヘルパー / 列挙子
- `src/webtransport/stream.rs` — `stream_type()` 関数
- `src/stream/buffer.rs` — `SendBuffer` / `RecvBuffer` の未使用メソッド
- `src/webtransport/flow_control.rs` — `WtFlowControl::Default` 実装
- `tests/test_stream/buffer.rs` — `RecvBuffer::take` の唯一の使用箇所 (書き換え対象)
