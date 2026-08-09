# 未使用コードを一括削除する

- Priority: Medium
- Created: 2026-06-11
- Completed: 2026-08-09
- Polished: 2026-08-08
- Model: deepseek-v4-pro
- Branch: feature/change-remove-unused-code

## 目的

コードベース内の未使用コード (死にコード・呼び出しのない関数・未使用の `Default` 実装等) を一括削除する。grep で全コードベース (`src/` / `crates/` / `tests/` / `pbt/` / `fuzz/` / `examples/`) を確認した結果、以下の 9 項目が未使用または不要であることが判明している。

なお、同じく未使用の公開 API として `Stream::recv_buffer()` / `Stream::recv_buffer_mut()` (`src/stream.rs`) と `SendBuffer::default()` / `RecvBuffer::default()` (`#[derive(Default)]` 由来) が存在するが、これらは削除対象に含めない。`recv_buffer` 系アクセサは将来の受信バッファ経路 API として意図的に維持し、`Default` derive は `max_size = 0` の空バッファを生成する設計として意図的なものと判断する。

## 優先度根拠

- 削除対象 9 項目はすべて canary.0 から公開済みの公開 API (未使用のため利用者は実質影響を受けないが、公開 API の削除は破壊的変更として扱う)。未使用 API を残したまま正式リリースすると、利用者が「使うべき API」と誤認するリスクがある
- 未使用 API は破壊的変更でしか削除できなくなるため、正式リリース前のこのタイミングを逃すと将来の互換性負債になる (0019 / 0071 の公開 API 削除と同じ扱い)
- 修正コストは低い (関数 / 実装の削除と一部テスト書き換え)
- 0068 が「`WtErrorKind::SessionClosed` 等は本 issue で削除予定」と前提にしているため、相対的に着手の優先度はある

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

**根拠**: 全コードベースで一度も呼ばれない。`src/webtransport/varint.rs` 内では `WtError::new(WtErrorKind::Incomplete)` が直接使われている。`WtErrorKind::Incomplete` 列挙子自体は `src/webtransport/varint.rs` の `decode` 系関数と `src/webtransport/capsule.rs` の `CapsuleDecoder::decode` で使用されており **維持** する。

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

**根拠**: 全コードベース (src / tests / pbt / fuzz / examples / crates) で一度も呼ばれない。`src/webtransport/varint.rs` の `encode` 関数で `WtErrorKind::BufferTooShort` を `WtError::with_reason` で構築している。`WtErrorKind::BufferTooShort` 列挙子自体は維持する。

### 3. `WtError::session_closed()` ヘルパーと `WtErrorKind::SessionClosed` 列挙子

**ファイル**: `src/webtransport/error.rs`
**削除範囲**:
- `WtErrorKind::SessionClosed` 列挙子定義 (`pub enum WtErrorKind` 内、doc コメントを含む)
- `impl std::fmt::Display for WtErrorKind` の対応 arm (`Self::SessionClosed => write!(f, "SessionClosed"),`)
- `WtError::session_closed()` ヘルパー関数定義 (`#[track_caller]` 属性行・doc コメントを含む)

```rust
/// セッションクローズエラーを生成する
#[track_caller]
pub fn session_closed<T: Into<String>>(reason: T) -> Self {
    Self::with_reason(WtErrorKind::SessionClosed, reason)
}
```

**根拠**: セッションクローズは状態遷移 (`WtSessionState::Closed`) とイベント (`WtEvent::SessionClosed`、`src/webtransport.rs` の `WtEvent` enum) の 2 経路で表現されており、いずれもエラー型を経由しない。エラー経路では `session_closed()` ヘルパーの呼び出しが存在しない。`WtErrorKind::SessionClosed` の参照元も `session_closed()` ヘルパーのみのため、列挙子と Display arm も同時に削除する (`crates/tokio-http2` の `wt_http2_error_code` は `_ => None` のワイルドカード arm を持つため、非網羅 match は発生しない)。

### 4. `stream_id::stream_type()` 関数

**ファイル**: `src/webtransport/stream.rs`
**削除範囲**: 関数定義 (`#[must_use]` 属性行・doc コメントを含む)

```rust
/// ストリームタイプを取得
#[must_use]
pub const fn stream_type(id: WtStreamId) -> u8 {
    (id & 0x03) as u8
}
```

**根拠**: 全コードベースで呼び出しなし。兄弟関数 `is_client_initiated()` / `is_bidirectional()` の組み合わせで等価に判定可能であり、`stream_type()` は冗長。`webtransport::stream::stream_id::stream_type` として外部公開されているが、未使用のため利用者への実質的な影響はない (公開 API の削除として [CHANGE] エントリには記載する)。

### 5. `SendBuffer::clear()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (doc コメントを含む)

**根拠**: 製品コード・テストコード・pbt・fuzz・examples・crates のすべてで呼び出しなし。

### 6. `RecvBuffer::clear()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (doc コメントを含む)

**根拠**: 製品コード・テストコード・pbt・fuzz・examples・crates のすべてで呼び出しなし。

### 7. `SendBuffer::remaining()` / `RecvBuffer::remaining()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (`#[must_use]` 属性行・doc コメントを含む)

**根拠**: 製品コード・テストコード・pbt・fuzz・examples・crates のすべてで呼び出しなし。0019 が「削除済み」と記録していたが実は残存していた API で、本 issue で改めて削除する。

### 8. `RecvBuffer::take()` メソッド

**ファイル**: `src/stream/buffer.rs`
**削除範囲**: メソッド定義 (doc コメントを含む) と関連テストの修正

**根拠**: 製品コードからの呼び出しはなく、`tests/test_stream/buffer.rs` の `test_recv_buffer_push_pop` / `test_recv_buffer_push_uses_saturating_add` でのみ使用されている。`pop(max_size)` で全データを取り出すなら `let len = buf.len(); buf.pop(len)` で等価の動作が得られるため、`take()` を削除して当該テストをこの形式に書き換える方針とする (可読性のため一時変数 `len` を経由する)。テスト自体は受信バッファの全データ取り出し動作の保証として有用なので削除せず維持する。`tests/test_stream/buffer.rs` の `buf2.take(); // 空にする` は書き換え後の no-op になるため、コメントを削除する。

### 9. `impl Default for WtFlowControl`

**ファイル**: `src/webtransport/flow_control.rs`
**削除範囲**: `impl Default for WtFlowControl` ブロック全体

**根拠**: 全コードベースで `WtFlowControl::default()` が一度も呼ばれていない。`WtFlowControl::new(...)` は `WtSession::new()` 内で `WtConfig` の値を引数として明示的に呼ばれており、`WtConfig::default()` 経由のセッション生成時にも `WtFlowControl::default()` を経由しない (`WtConfig::default()` の各フィールド値で `WtFlowControl::new(...)` が呼ばれる)。さらに `WtFlowControl::default()` の値 (`Self::new(1_048_576, 1_048_576, 100, 100, 100, 100)`) は `WtConfig::default()` の初期値を複製しただけの冗長実装であり、`WtConfig::default()` の値が将来変わったときに `WtFlowControl::default()` だけ取り残される保守性の問題も抱える。`Default` 実装の削除で動作変更は発生しない。

## CHANGES.md の扱い

削除対象 9 項目はすべて canary.0 から公開済みの公開 API のため、削除は `[CHANGE]` エントリとして `## develop` セクションに記載する。

- `## develop` セクションの既存 `[CHANGE]` 群の末尾 (最後の `[CHANGE]` エントリの直後) に以下のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネストする:

   ```markdown
   - [CHANGE] 未使用の公開 API を削除する (`WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` / `WtErrorKind::SessionClosed` / `stream_id::stream_type` / `SendBuffer::clear` / `RecvBuffer::clear` / `SendBuffer::remaining` / `RecvBuffer::remaining` / `RecvBuffer::take` / `WtFlowControl` の `Default` 実装)。`RecvBuffer::take` を使用していたテストは `let len = buf.len(); buf.pop(len)` 形式に書き換える
     - @voluntas
   ```

   `WtErrorKind::Incomplete` / `WtErrorKind::BufferTooShort` 列挙子および `WtFlowControl::new` は維持するため、エントリには含めない。

## 他 issue との関係

- **0068 (`bug-fix-wt-error-design`)**: 本 issue は 0068 マージ後にマージされる前提。0068 のテストでは `WtErrorKind::SessionClosed` / `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` を使用しないことが既に明記されている。0068 マージ後に `WtErrorKind::SessionClosed` の参照元は本 issue で削除される `WtError::session_closed()` のみとなる。`CHANGES.md` を編集するため、コンフリクトの可能性がある (内容は異なる箇所なので 3-way merge で解決できる見込み)。なお 0068 の優先度根拠に「issue 0072 が扱う未リリース API の削除」という記述があるが、本 issue の削除対象は canary.0 から公開済みの公開 API であり、0068 側の表現はカテゴリ変更前の記述が残っているものである
- **0070 (`change-privatize-error-wt-error-fields`)**: 0070 → 0072 / 0072 → 0070 のどちらの順序でマージしても衝突しない。0070 のテストでも本 issue で削除予定の API は使用しない。`skills/shiguredo-http2/SKILL.md` と `CHANGES.md` を編集するため、コンフリクトの可能性がある
- **0071 (`change-remove-send-error`)**: `SendError` 削除の独立 issue。0071 も `skills/shiguredo-http2/SKILL.md` と `CHANGES.md` を編集するため、コンフリクトの可能性がある (内容は異なる箇所なので 3-way merge で解決できる見込み)
- **0073 (`change-rfc9297-non-minimal-varint`)**: `varint.rs` を改修する可能性があり、本 issue が削除する `WtError::incomplete` ヘルパーは `varint.rs` で使用されていないため独立。`WtErrorKind::Incomplete` 列挙子は本 issue で維持する。`CHANGES.md` は編集しないためコンフリクトなし
- **0076 / 0078 / 0102 / 0103**: それぞれ `CHANGES.md` を編集するため、コンフリクトの可能性がある (内容は異なる箇所なので 3-way merge で解決できる見込み)

注記: 0070 / 0071 / 0073 は本 issue を旧ブランチ名 `refactor-remove-unused-code` で参照しているが、本 issue は公開 API 削除のためカテゴリを `change` (`feature/change-remove-unused-code`) に変更した。各 issue の参照は磨き上げ時に更新する。

順序依存は 0068 → 0072 のみ。

## 単一カテゴリ性について

9 項目はそれぞれ異なるサブ領域 (WtError 系 / WebTransport ストリーム ID / HTTP/2 ストリームバッファ / WebTransport フロー制御) にまたがるが、「未使用の公開 API を削除する」という単一の論理目的に集約している。先行事例 0019 (`feature/change-remove-dead-code`) も複数領域 (frame encoder / frame decoder / frame flags / stream state) をまたいで「未使用コード一括削除」として 1 issue で扱った前例があり、本 issue もこれに倣う。公開 API の削除は破壊的変更のため、カテゴリは `change` を採用する。

## 変更対象ファイル一覧

### 編集するファイル

- `src/webtransport/error.rs` — `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` / `WtErrorKind::SessionClosed` 削除、`Display` arm 削除
- `src/webtransport/stream.rs` — `stream_id::stream_type` 関数削除
- `src/stream/buffer.rs` — `SendBuffer::clear` / `RecvBuffer::clear` / `SendBuffer::remaining` / `RecvBuffer::remaining` / `RecvBuffer::take` 削除
- `src/webtransport/flow_control.rs` — `impl Default for WtFlowControl` 削除
- `tests/test_stream/buffer.rs` — `test_recv_buffer_push_pop` / `test_recv_buffer_push_uses_saturating_add` 内の `buf.take()` を `let len = buf.len(); buf.pop(len)` 形式に書き換え。`test_recv_buffer_push_uses_saturating_add` の `buf2` ブロックは `push → pop で全取り出し → 再度 push 成功` の経路を検証する形に書き換え
- `skills/shiguredo-http2/SKILL.md` — `WtErrorKind` のバリアント一覧から `SessionClosed` を除去
- `CHANGES.md` — `[CHANGE]` エントリ 1 件追加

## 対応手順

1. 作業ブランチ `feature/change-remove-unused-code` を作成する。本 issue は 0068 が `src/webtransport/error.rs` の Display/Debug を改修するため、0068 マージ後の develop からブランチを切る前提とする
2. `src/webtransport/error.rs` の `WtError::incomplete()` / `WtError::buffer_too_short()` / `WtError::session_closed()` を `#[track_caller]` 属性行と doc コメントごと削除する
3. `src/webtransport/error.rs` の `WtErrorKind::SessionClosed` 列挙子 (doc コメントを含む) と、`impl std::fmt::Display for WtErrorKind` の `Self::SessionClosed => write!(f, "SessionClosed"),` arm を削除する
4. `src/webtransport/stream.rs` の `stream_id::stream_type()` 関数を `#[must_use]` 属性行と doc コメントごと削除する
5. `src/stream/buffer.rs` の `SendBuffer::clear` / `RecvBuffer::clear` / `SendBuffer::remaining` / `RecvBuffer::remaining` / `RecvBuffer::take` メソッドを doc コメントごと削除する
6. `tests/test_stream/buffer.rs` の `test_recv_buffer_push_pop` / `test_recv_buffer_push_uses_saturating_add` 内の `buf.take()` を `let len = buf.len(); buf.pop(len)` 形式に書き換える。`test_recv_buffer_push_uses_saturating_add` の `buf2.take(); // 空にする` は構築直後の空バッファに対する no-op になるため、同ブロックを `push → pop で全取り出し → 再度 push 成功` の経路を検証する形に書き換えて `buf` 側と重複させない
7. `src/webtransport/flow_control.rs` の `impl Default for WtFlowControl` ブロックを削除する
8. `skills/shiguredo-http2/SKILL.md` の `WtErrorKind` バリアント一覧から `SessionClosed` を除去する (`WtEvent::SessionClosed` (イベント variant) は維持対象のため誤って削除しない)
9. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の末尾に「CHANGES.md の扱い」で示した `[CHANGE]` エントリと担当者行を追加する
10. `cargo fmt --all -- --check` で整形違反がないことを確認する
11. `cargo build --workspace` でビルドが成功することを確認する
12. `cargo test --workspace` で全テスト通過を確認する
13. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
14. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `src/webtransport/error.rs` から `WtError::incomplete` / `WtError::buffer_too_short` / `WtError::session_closed` / `WtErrorKind::SessionClosed` (および対応する `Display` arm) が削除されている
- `src/webtransport/stream.rs` から `stream_id::stream_type` 関数が削除されている
- `src/stream/buffer.rs` から `SendBuffer::clear` / `RecvBuffer::clear` / `SendBuffer::remaining` / `RecvBuffer::remaining` / `RecvBuffer::take` が削除されている
- `tests/test_stream/buffer.rs` の `buf.take()` 呼び出しが `let len = buf.len(); buf.pop(len)` 形式に書き換えられ、`test_recv_buffer_push_uses_saturating_add` の `buf2` ブロックが `push → pop で全取り出し → 再度 push 成功` の経路を検証する形に書き換えられている
- `src/webtransport/flow_control.rs` から `impl Default for WtFlowControl` が削除されている
- `skills/shiguredo-http2/SKILL.md` の `WtErrorKind` バリアント一覧から `SessionClosed` が除去されている (`WtEvent::SessionClosed` は維持されている)
- `CHANGES.md` の `## develop` に削除対象 9 項目を記載した `[CHANGE]` エントリ 1 件と担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo build --workspace` が成功する
- `cargo test --workspace` が成功する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `issues/closed/0019-chore-remove-dead-code.md` — 過去の未使用コード一括削除の先行事例 (`SendBuffer::remaining` / `clear` 等を「削除済み」と記録しているが、実際には残存しており本 issue が改めて削除する)
- `issues/0068-bug-fix-wt-error-design.md` — 0068 が本 issue を「`WtErrorKind::SessionClosed` 等は本 issue で削除予定」と前提にしている
- `issues/0071-change-remove-send-error.md` — `SendError` 削除の同種の公開 API 削除事例
- `src/webtransport/error.rs` — `WtError` / `WtErrorKind` の未使用ヘルパー / 列挙子
- `src/webtransport/stream.rs` — `stream_type()` 関数
- `src/stream/buffer.rs` — `SendBuffer` / `RecvBuffer` の未使用メソッド
- `src/webtransport/flow_control.rs` — `WtFlowControl::Default` 実装
- `tests/test_stream/buffer.rs` — `RecvBuffer::take` の唯一の使用箇所 (書き換え対象)
- `skills/shiguredo-http2/SKILL.md` — `WtErrorKind` バリアント一覧の `SessionClosed` 除去対象

## 解決方法

### 削除対象 9 項目の削除

`src/webtransport/error.rs` から `WtError::incomplete()` / `WtError::buffer_too_short()` / `WtError::session_closed()` ヘルパーと `WtErrorKind::SessionClosed` 列挙子 (Display arm 含む) を、`src/webtransport/stream.rs` から `stream_id::stream_type()` を、`src/stream/buffer.rs` から `SendBuffer::clear()` / `RecvBuffer::clear()` / `SendBuffer::remaining()` / `RecvBuffer::remaining()` / `RecvBuffer::take()` を、`src/webtransport/flow_control.rs` から `impl Default for WtFlowControl` を削除した。いずれも全コードベースで呼び出しが存在しないことを grep で確認済み。

### テスト書き換え

`tests/test_stream/buffer.rs` の `RecvBuffer::take()` 使用箇所を `let len = buf.len(); buf.pop(len)` 形式に書き換えた。`test_recv_buffer_push_pop` は全取り出し + 空化の検証を維持し、`test_recv_buffer_push_uses_saturating_add` は `push → pop で全取り出し → is_empty 確認 → 再度 push 成功` の経路を 1 つのバッファで検証する形に統合した。

### SKILL.md と CHANGES.md

`skills/shiguredo-http2/SKILL.md` の `WtErrorKind` バリアント一覧から `SessionClosed` を除去した (`WtEvent::SessionClosed` イベント variant は維持)。`CHANGES.md` の `## develop` に削除対象 11 個の API を列挙した `[CHANGE]` エントリを追加した。

### 検証

`cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo check --manifest-path fuzz/Cargo.toml` のすべてが通過することを確認した。

### 備考

本 issue の本文では `WtFlowControl::default()` の値が `WtConfig::default()` の初期値を複製したものと記載しているが、実際の `WtConfig::default()` の値 (`initial_max_stream_data_* = 262_144`) とは一致しない。削除の妥当性 (呼び出しゼロ) には影響しない記録上のズレであり、むしろ将来 `WtConfig::default()` と異なる値になる潜在バグが除去された。
