# 未使用コードを一括削除する

- Priority: Medium
- Created: 2026-06-11
- Polished: 2026-06-16
- Model: deepseek-v4-pro
- Branch: feature/change-remove-unused-code

注: ファイル名は `0072-refactor-remove-unused-code.md` だが、削除対象に canary リリースに含まれる公開 API (`SendBuffer::clear` / `RecvBuffer::clear` / `RecvBuffer::take` / `SendBuffer::remaining` / `RecvBuffer::remaining`) が含まれるため、ブランチ命名は `feature/change-` を採用する (`shiguredo-git` 規約: 後方互換なし → `change`)。issue ファイル名のリネーム要否はユーザー判断とする。

## 目的

コードベース内の未使用コード (死にコード・呼び出しのない関数・未使用の `Default` 実装等) を一括削除する。`grep -rE` で全コードベース (`src/` / `crates/` / `tests/` / `pbt/` / `fuzz/` / `examples/`) を確認した結果、以下の 10 項目が未使用または不要であることが判明している。内訳: WtError 系 3 + `WtErrorKind::SessionClosed` 1 + WebTransport ストリーム ID 関数 1 + HTTP/2 ストリームバッファ 5 + WebTransport フロー制御 Default 1。

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
- `impl std::fmt::Display for WtErrorKind` の対応 arm (`Self::SessionClosed => write!(f, "SessionClosed"),`)。**注**: 削除するのは `impl Display for WtErrorKind` の arm であり、`impl Display for WtError` (0068 で変更) ではない。両者は別 impl
- `WtError::session_closed()` ヘルパー関数定義 (`#[track_caller]` 属性行・doc コメントを含む)

```rust
/// セッションクローズエラーを生成する
#[track_caller]
pub fn session_closed<T: Into<String>>(reason: T) -> Self {
    Self::with_reason(WtErrorKind::SessionClosed, reason)
}
```

**根拠**: セッションクローズは状態遷移 (`WtSessionState::Closed`) で表現されており、エラー経路では `session_closed()` ヘルパーの呼び出しが存在しない。`WtErrorKind::SessionClosed` の参照元も `session_closed()` ヘルパーのみのため、列挙子と Display arm も同時に削除する。`WtEvent::SessionClosed { error_code, reason }` (`src/webtransport/mod.rs`) は別物 (WebTransport セッションのクローズイベント) であり、本 issue では削除しない。Display arm 削除は `match` の網羅性チェックでコンパイル時保証されるためテスト追加不要。

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

**根拠**: 製品コードからの呼び出しはなく、`tests/test_stream/buffer.rs:29,56` (`test_recv_buffer_push_pop` / `test_recv_buffer_push_uses_saturating_add`) でのみ使用されている。書き換え方針:
- `tests/test_stream/buffer.rs:29` (`buf.take()`): `pop(buf.len())` 形式に書き換え (実データを取り出している)
- `tests/test_stream/buffer.rs:56` (`buf2.take()`): 直前 L55 が `RecvBuffer::new(usize::MAX)` 直後の呼び出しでバッファ空のため意味的に no-op。**行ごと削除**する

テスト関数自体は受信バッファの動作保証として有用なので関数自体は維持する。

### 8. `SendBuffer::remaining()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (doc コメントを含む)

**根拠**: 全コードベースで呼び出し 0 件 (grep 確認済み)。`canary.6` リリースに含まれる公開 API のため、削除は `[CHANGE]` 区分。

### 9. `RecvBuffer::remaining()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (doc コメントを含む)

**根拠**: 全コードベースで呼び出し 0 件 (grep 確認済み)。`canary.6` リリースに含まれる公開 API のため、削除は `[CHANGE]` 区分。

### 10. `impl Default for WtFlowControl`

**ファイル**: `src/webtransport/flow_control.rs`
**削除範囲**: `impl Default for WtFlowControl` ブロック全体 (doc コメントは存在しない)

**根拠**: 全コードベースで `WtFlowControl::default()` が一度も呼ばれていない。`WtFlowControl::new(...)` は `WtSession::new()` 内で `WtConfig` の値を引数として明示的に呼ばれており、`WtConfig::default()` 経由のセッション生成時にも `WtFlowControl::default()` を経由しない (`WtConfig::default()` の各フィールド値で `WtFlowControl::new(...)` が呼ばれる)。`WtSession` は `#[derive(Debug)]` のみで `Default` 派生していないため、`WtFlowControl::default()` が暗黙呼び出される経路はない (`WtSessionState` は `Default` 派生だが、これは `flow_control` フィールドとは無関係)。`Default` 実装の削除で動作変更は発生しない。

## CHANGES.md の扱い

削除対象 10 項目のリリース状況判定:

- **draft-ietf-webtrans-http2 対応として develop で追加された未リリース API** (`WtError` 系 #1〜#3 + #4 `WtErrorKind::SessionClosed` 部分 + #5 `stream_type` + #10 `WtFlowControl::Default`): draft-ietf-webtrans-http2-14 対応の commit で追加されたため canary.X の現バージョン (`Cargo.toml` の `version = "2026.1.0-canary.6"`) には含まれない。`shiguredo-changelog` 規約「派生元ブランチとの最終的な差分のみ記載」に従い、CHANGES.md 編集不要
- **canary.X リリースに既に含まれる公開 API** (`SendBuffer::clear` #6 / `RecvBuffer::clear` #7 / `RecvBuffer::take` #8 / `SendBuffer::remaining` #9 / `RecvBuffer::remaining`): 初期コミット `f6f2286 インポート` から存在し canary.0 以降のリリースに含まれる。canary タグは事前リリースだが既に公開された変更履歴上の状態のため、`CHANGES.md` の `## develop` セクションに `[CHANGE]` エントリ 1 件を追加する (`shiguredo-issues` 規約により issue 番号は含めない)

CHANGES.md エントリ文言案:

```markdown
- [CHANGE] 製品コードで未使用の公開 API (`SendBuffer::clear`, `RecvBuffer::clear`, `RecvBuffer::take`, `SendBuffer::remaining`, `RecvBuffer::remaining`) を削除する。`RecvBuffer::take` を使用していたテストは `pop(buf.len())` 形式に書き換えまたは行削除する
  - @voluntas
```

参考: `issues/closed/0019-chore-remove-dead-code.md` L63 「`SendBuffer::clear`, `RecvBuffer::clear` (削除済み)」は 0019 closed 時の記述だが、現リポジトリで両メソッドは `src/stream/buffer.rs:63,128` に **存在する**。本 issue はこの不一致 (0019 では削除提案されたが何らかの経緯で再追加された、もしくは 0019 closed 文書の記述が誤り) に対する再削除でもある。0019 の git log を辿っても再追加 commit は特定できなかったため、本 issue マージ後に意図的な再追加 PR が起こらないよう、CHANGES.md エントリで明確に削除した旨を残す。

## 他 issue との関係

- **0068 (`bug-fix-wt-error-display-info-leak`)**: 本 issue は 0068 マージ後にマージされる前提。0068 のテストでは `WtErrorKind::SessionClosed` / `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` を使用しないことが既に明記されている。0068 マージ後に `WtErrorKind::SessionClosed` の参照元は本 issue で削除される `WtError::session_closed()` のみとなる
- **0069 (`bug-fix-nghttp2-send-set-user-data`)**: `shiguredo_nghttp2` 内の修正で、`shiguredo_http2` の `WtError` 等とは無関係。順序依存なし
- **0070 (`change-privatize-error-wt-error-fields`)**: 0068 マージ後であれば、0070 → 0072 / 0072 → 0070 のどちらの順序でマージしても衝突しない。0070 のテストでも本 issue で削除予定の API は使用しない
- **0071 (`refactor-remove-send-error`)**: `SendError` 削除の独立 issue。コンフリクトなし
- **0077 (`change-tokio-http2-error-add-webtransport-variant`)**: 0072 で `WtErrorKind::SessionClosed` を削除するため、0077 の単体テストでは削除されない `WtErrorKind::Incomplete` を使用するよう調整済み。0072 → 0077 の順序でマージしてもコンパイルエラーにならない
- **0073 (`change-rfc9297-non-minimal-varint`)**: `varint.rs` を改修する可能性があり、本 issue が削除する `WtError::incomplete` ヘルパーは `varint.rs` で使用されていないため独立。`WtErrorKind::Incomplete` 列挙子は本 issue で維持する
- **0074-0076**: それぞれ無関係

順序依存は 0068 → 0072 のみ。

## 1 issue / 1 branch にまとめる根拠

10 項目はそれぞれ異なるサブ領域 (WtError 系 / WebTransport ストリーム ID / HTTP/2 ストリームバッファ / WebTransport フロー制御) にまたがるが、「未使用の公開 API を削除する」という単一の論理目的に集約している。先行事例 0019 (`chore-remove-dead-code`) も複数領域 (frame encoder / decoder / stream_state) をまたいで「未使用コード一括削除」として 1 issue で扱った前例があり、本 issue もこれに倣う。CHANGES.md 上で `[CHANGE]` が必要な canary 同梱項目 (#6〜#9 のうち実体は 5 件、内訳は CHANGES.md セクション参照) と changelog 不要の未リリース項目が混在するが、最終的な変更履歴では 1 行の `[CHANGE]` エントリにまとまるためレビュー単位として一貫している。

## 変更対象ファイル一覧

### 編集するファイル

- `src/webtransport/error.rs` — `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` / `WtErrorKind::SessionClosed` 削除、`impl Display for WtErrorKind` の `SessionClosed` arm 削除
- `src/webtransport/stream.rs` — `stream_id::stream_type` 関数削除 (stream type 説明テーブル `stream.rs:1-13` は `is_bidirectional()` / `is_client_initiated()` の組合せ説明として有用なため維持する)
- `src/stream/buffer.rs` — `SendBuffer::clear` / `RecvBuffer::clear` / `RecvBuffer::take` / `SendBuffer::remaining` / `RecvBuffer::remaining` 削除
- `src/webtransport/flow_control.rs` — `impl Default for WtFlowControl` 削除
- `tests/test_stream/buffer.rs` — `test_recv_buffer_push_pop` 内の `buf.take()` を `pop(buf.len())` 形式に書き換え、`test_recv_buffer_push_uses_saturating_add` 内の `buf2.take()` (`buf2.len() == 0` の意味的 no-op) を行ごと削除
- `CHANGES.md` — `[CHANGE]` エントリ 1 件追加

注: 行番号 (varint.rs:71,144,158,166,177、capsule.rs:333,344) は補助情報。0073 マージ後等で位置が変動する可能性があるため、最新位置は `grep -n WtErrorKind::Incomplete src/webtransport/varint.rs` で再確認する。

## 対応手順

1. 作業ブランチ `feature/change-remove-unused-code` を作成する
2. `src/webtransport/error.rs` の `WtError::incomplete()` / `WtError::buffer_too_short()` / `WtError::session_closed()` を `#[track_caller]` 属性行と doc コメントごと削除する
3. `src/webtransport/error.rs` の `WtErrorKind::SessionClosed` 列挙子と、`impl std::fmt::Display for WtErrorKind` の `Self::SessionClosed => write!(f, "SessionClosed"),` arm を削除する
4. `src/webtransport/stream.rs` の `stream_id::stream_type()` 関数を doc コメントごと削除する (`stream.rs:1-13` の説明テーブルは維持)
5. `tests/test_stream/buffer.rs` の `buf.take()` を `pop(buf.len())` 形式に書き換え、`buf2.take()` を行ごと削除する (中間状態でのコンパイルエラーを避けるため、`buffer.rs` の削除より先に実施)
6. `src/stream/buffer.rs` の `SendBuffer::clear` / `RecvBuffer::clear` / `RecvBuffer::take` / `SendBuffer::remaining` / `RecvBuffer::remaining` メソッドを doc コメントごと削除する
7. `src/webtransport/flow_control.rs` の `impl Default for WtFlowControl` ブロックを削除する
8. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の末尾に上記の `[CHANGE]` エントリと担当者行を追加する (`shiguredo-issues` 規約により issue 番号は含めない)
9. `cargo fmt --all -- --check` で整形違反がないことを確認する
10. `cargo test --workspace` で全テスト通過を確認する (test は内部でビルドも兼ねるため `cargo build` は省略)
11. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
12. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `src/webtransport/error.rs` から `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` / `WtErrorKind::SessionClosed` (および `impl Display for WtErrorKind` の対応 arm) が削除されている
- `src/webtransport/stream.rs` から `stream_id::stream_type` 関数が削除されている
- `src/stream/buffer.rs` から `SendBuffer::clear` / `RecvBuffer::clear` / `RecvBuffer::take` / `SendBuffer::remaining` / `RecvBuffer::remaining` が削除されている
- `tests/test_stream/buffer.rs` の `buf.take()` が `pop(buf.len())` 形式に書き換えられ、`buf2.take()` (no-op) が行ごと削除されている
- `src/webtransport/flow_control.rs` から `impl Default for WtFlowControl` が削除されている
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリ 1 件と担当者行が追加されている (issue 番号なし)
- `cargo fmt --all -- --check` が通過する
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
