# PBT ファイル命名規約と配置構造の是正

Created: 2026-05-23
Completed: 2026-05-24
Model: Opus 4.7
Branch: feature/refactor-pbt-naming-convention

## 内容

CLAUDE.md「テストについて」(L78-L88) の以下 3 規約に違反する PBT ファイルを是正する。

```
- PBT のファイル名は pbt/tests/prop_<module>.rs とし、src/<module>.rs に対応させること
- 特定のモジュールに対応しないテストには test_ や prop_ プレフィックスを付けないこと
- src/<module>/ のようにディレクトリモジュールの場合は pbt/tests/prop_<module>/main.rs にサブモジュール対応で分割すること
```

issue タイトルは「命名規約違反」だが実態は **ファイル命名 + 配置構造 (ディレクトリモジュール対応)** の両面の是正。

## 現状棚卸し

`pbt/tests/` 配下の全 12 ファイルと対応モジュールの整合性を一覧化する。

| 現状 PBT ファイル | 対応 `src/` モジュール | 規約準拠状態 | 是正後の配置 |
|---|---|---|---|
| `prop_connection.rs` | `src/connection/mod.rs` (dir module、配下は `mod.rs` のみ) | 準拠 (L88 は dir 内サブモジュールが存在する場合の規定。`src/connection/` は実質単一ファイルのため L83 単一 file 規約で OK) | 無変更 |
| `prop_frame.rs` | `src/frame/` (`decoder.rs`, `encoder.rs`, `error.rs`, `flags.rs`, `mod.rs`) | **要是正** | `prop_frame/main.rs` + 各サブモジュール対応 |
| `prop_hpack.rs` | `src/hpack/` (8 サブモジュール) | **要是正** | `prop_hpack/main.rs` + 各サブモジュール |
| `prop_webtransport.rs` | `src/webtransport/` (`mod.rs`, `stream.rs`, `capsule.rs`, `flow_control.rs`, `varint.rs`) | **要是正** | `prop_webtransport/main.rs` + 各サブモジュール |
| `prop_dynamic_table.rs` | `src/hpack/dynamic_table.rs` (`src/hpack/` 配下のサブモジュール) | **要是正** (規約 3 サブモジュール対応) | `prop_hpack/dynamic_table.rs` (`prop_hpack/main.rs` に取り込み) |
| `prop_stream_state.rs` | `src/stream/state.rs` (dir module 内サブモジュール) | **要是正** | `prop_stream/state.rs` に `git mv` + `prop_stream/main.rs` を **新規作成** (中身は `mod state;` のみのスケルトン) |
| `prop_header_field_syntax.rs` | 対応 `src/` モジュール無し (0024 で `bytes.rs` と `table.rs` 横断のため命名した) | **要是正** (0034 で `src/syntax.rs` 新設後に rename) | `prop_syntax.rs` (0034 blocking) |
| `prop_validation.rs` | `src/validation.rs` (single file) | 準拠 | 無変更 |
| `prop_settings.rs` | `src/settings.rs` (single file) | 準拠 | 無変更 |
| `prop_event.rs` | `src/event.rs` (single file) | 準拠 | 無変更 |
| `prop_flow_control.rs` | `src/flow_control.rs` (single file) + `src/webtransport/flow_control.rs` (重複モジュール名) | **衝突**: 名前競合 | `prop_flow_control.rs` を維持 (`src/flow_control.rs` 対応) + 将来 `src/webtransport/flow_control.rs` 用は `prop_webtransport/flow_control.rs` に分離 |
| `prop_error.rs` | `src/error.rs` (single file) | 準拠 | 無変更 |

棚卸し結果: 12 ファイル中 **6 ファイルを是正 (うち 4 ファイルはディレクトリ化、1 ファイルは別ディレクトリへ移管 + サブモジュール化、1 ファイルは rename)**、5 ファイルは準拠で無変更 (`prop_validation.rs`, `prop_settings.rs`, `prop_event.rs`, `prop_error.rs`, `prop_connection.rs`)、`prop_flow_control.rs` は衝突方針確定で現状維持。

## 設計方針

### Phase 分割 (実装手順の推奨)

12 ファイルの一括 rename は diff が膨大でレビュー困難。実装者裁量で以下 Phase に分割可能。

- **Phase A**: `prop_header_field_syntax.rs` → `prop_syntax.rs` rename (0034 完了後の唯一の作業)。1 ファイル
- **Phase B**: `prop_hpack.rs` / `prop_dynamic_table.rs` を `prop_hpack/` ディレクトリにまとめる
- **Phase C**: `prop_stream_state.rs` を `prop_stream/state.rs` に移管 + `prop_stream/main.rs` 新設
- **Phase D**: `prop_frame.rs`, `prop_webtransport.rs` をディレクトリ化 (`src/frame/` および `src/webtransport/` にサブモジュールが存在し L88 の「サブモジュール対応で分割」の対象となる)。本 issue では既存ファイルを `prop_<module>/main.rs` に rename するのみで、サブモジュール単位への中身分割は別 issue。`prop_connection.rs` は `src/connection/` 配下がサブモジュール無しのため L83 単一 file 規約準拠で **対象外**

Phase 別 PR で進める場合のブランチ命名: `feature/refactor-pbt-naming-phase-a` 等。1 PR にまとめる場合は `feature/refactor-pbt-naming-convention`。

### ディレクトリモジュール化の具体構造

`pbt/Cargo.toml` には `[[test]]` 明示宣言がなく Cargo の auto-discovery (`tests/` 配下の `.rs` ファイルおよび `tests/foo/main.rs` ディレクトリ) に依存している。`pbt/tests/prop_hpack/main.rs` 形式に変えても Cargo は自動的に integration test として認識する。`pbt/Cargo.toml` への明示宣言追加は不要。

例: `prop_hpack/` ディレクトリの構成 (Phase B)

```
pbt/tests/prop_hpack/
├── main.rs            # mod 宣言とテスト共通設定 (mod dynamic_table;)
└── dynamic_table.rs   # 旧 prop_dynamic_table.rs の内容
```

`prop_hpack/main.rs` の中身は以下のスケルトン (既存 `prop_hpack.rs` の内容を移植 + サブモジュール宣言):

```rust
//! HPACK モジュール群の PBT
//!
//! `src/hpack/` ディレクトリモジュール配下のサブモジュールに対応する PBT を集約する。

mod dynamic_table;

use proptest::prelude::*;
use shiguredo_http2::{HeaderField, HpackDecoder, HpackEncoder};

// 既存 prop_hpack.rs の proptest! ブロック群
```

サブモジュールが追加されていない (`prop_connection/`, `prop_frame/`, `prop_webtransport/`) 場合は `mod` 宣言のみ無しで `main.rs` 1 ファイル構成にする。将来サブモジュール追加時は `mod` 宣言を追記する。

### `prop_header_field_syntax.rs` → `prop_syntax.rs` (Phase A)

0034 で `src/syntax.rs` が新設される。ファイル内容 (M5 同値性 PBT) は不変、ファイル名のみ rename。`pbt/tests/` 直下の単一ファイルとして配置 (`src/syntax.rs` も単一ファイルのため `pbt/tests/prop_syntax.rs` で規約準拠)。

### `prop_flow_control.rs` の衝突方針

CLAUDE.md L83 「PBT のファイル名は `pbt/tests/prop_<module>.rs`」は **同じモジュール名が複数 src パスに存在する場合の扱いを明示していない**。本 issue では以下の方針を採る。

- `pbt/tests/prop_flow_control.rs` は **`src/flow_control.rs` (接続/ストリームレベル) 対応**として維持する
- `src/webtransport/flow_control.rs` 用 PBT (現状未作成) を将来書くときは `pbt/tests/prop_webtransport/flow_control.rs` 形式 (Phase D の `prop_webtransport/` ディレクトリ配下) に置く
- これにより top-level の `prop_<name>.rs` は crate top-level の `src/<name>.rs` に、ディレクトリ配下の `prop_<dir>/<name>.rs` は `src/<dir>/<name>.rs` に対応する一貫性が保たれる
- この方針を `prop_flow_control.rs` 冒頭の doc コメントに明記する

### スコープ外: 各 PBT 内 `use` 文の整理

ディレクトリ化に伴い既存 PBT 内の `use shiguredo_http2::hpack::DynamicTable;` 等の crate path は変わらない (crate path はファイル位置と無関係)。ファイル移動と `mod` 宣言追加のみで完結し、`use` 文書き換えは原則発生しない。

## 完了条件

- [x] Phase A: `pbt/tests/prop_header_field_syntax.rs` は 0045 で crate 内 `#[cfg(test)]` に移管済みのため削除されている (事前に削除確認済み)
- [x] Phase B: `pbt/tests/prop_hpack/main.rs` + `pbt/tests/prop_hpack/dynamic_table.rs` 構成になっており、旧 `prop_hpack.rs` と `prop_dynamic_table.rs` は削除されている。`main.rs` 内に `mod dynamic_table;` が宣言されている
- [x] Phase C: `pbt/tests/prop_stream/state.rs` (旧 `prop_stream_state.rs` を `git mv` で移動) + `pbt/tests/prop_stream/main.rs` (新規作成、中身は doc コメント + `mod state;` のみのスケルトン) 構成になっている。旧 `prop_stream_state.rs` は移動完了で物理消失
- [x] Phase D: `pbt/tests/prop_frame/main.rs`, `pbt/tests/prop_webtransport/main.rs` 構成になっており、旧 single file 版は `git mv` でディレクトリ配下に移されている。本 issue のスコープでは **既存ファイル全体を `main.rs` に rename するのみ** とし、`src/frame/` / `src/webtransport/` 配下のサブモジュール (decoder/encoder/error/flags、stream/capsule/flow_control/varint) ごとの分割は **別 issue** に委ねる
- [x] `prop_connection.rs` は無変更 (`src/connection/` がサブモジュール無しの dir module のため L83 単一 file 規約準拠で OK)
- [x] `git mv issues/0039-refactor-pbt-naming-convention.md issues/0039-refactor-pbt-naming-convention.md` の rename と、それを参照する wikilink (`issues/0034-...` および `issues/0036-...` 内の `[[0039-refactor-pbt-naming-convention]]`) の更新を **同一 PR / 同一コミット** で行う (`git grep -l '0039-refactor-pbt-naming-convention' issues/` で対象を確認可能)。Phase B-D を別 PR で先行する場合でも、ファイル名 rename と wikilink 更新は分離せず一括で実施する
- [x] `prop_dynamic_table.rs` / `prop_stream_state.rs` / `prop_hpack.rs` / `prop_header_field_syntax.rs` / `prop_frame.rs` / `prop_webtransport.rs` の rename / 移動には `git mv` を使用し履歴を保持する
- [x] `pbt/tests/` 直下の `prop_*` 以外の dir / file (現状存在しないが将来追加されうる `common/` 等) は本 issue の影響を受けないことを PR レビュー時に確認する
- [x] `git mv issues/0039-refactor-pbt-naming-convention.md issues/0039-refactor-pbt-naming-convention.md` で category を `fix-` から `refactor-` に変更する (CLAUDE.md L41-L45 の category 規約と内容実態 = リファクタリングを整合させる)
- [x] `pbt/tests/prop_flow_control.rs` 冒頭 doc コメントに「本 PBT は `src/flow_control.rs` (接続/ストリームレベル) 対応。`src/webtransport/flow_control.rs` 用 PBT は将来 `prop_webtransport/flow_control.rs` に置く」旨が記載されている
- [x] `prop_validation.rs`, `prop_settings.rs`, `prop_event.rs`, `prop_error.rs`, `prop_flow_control.rs` の 5 ファイルは無変更
- [x] 移行前後で `cargo test --workspace` の passed 件数が一致する (`cargo test --workspace 2>&1 | grep "^test result:" | awk '{p+=$4} END {print p}'` で集計)
- [x] `cargo llvm-cov report` の PBT 経路カバレッジが移行前後で同等以上
- [x] `cargo build` が通る
- [x] `cargo clippy --workspace --all-targets -- -D warnings` が通る
- [x] `cargo fmt --all -- --check` が通る
- [x] CHANGES.md `### misc` に下記文面を追記

## CHANGES.md エントリ

`## develop` の `### misc` に追記する:

```
- [UPDATE] PBT ファイルの命名と配置を CLAUDE.md 規約 (`pbt/tests/prop_<module>.rs` および dir module 用 `prop_<module>/main.rs` 形式) に整合させる
  - @voluntas
```

## ブランチ命名

1 PR にまとめる場合: `feature/refactor-pbt-naming-convention`
Phase 別 PR にする場合: `feature/refactor-pbt-naming-phase-{a..d}`

## スコープ外

- 既存 PBT の **テストロジック変更** → 本 issue はファイル名と配置の rename のみ。テストロジック・assert・strategy は無変更
- 新規 PBT の追加 → 別 issue (0038 で `prop_concatenate_cookies` を追加する等)
- `src/__test_helpers.rs` → 0045 で廃止済み
- `src/webtransport/flow_control.rs` 用 PBT 新規追加 → 将来別 issue (本 issue は配置方針のみ確定)
- `pbt/Cargo.toml` の `[[test]]` 明示宣言追加 → Cargo auto-discovery で動作するため不要
- `prop_frame/main.rs` および `prop_webtransport/main.rs` の **中身を `src/frame/` / `src/webtransport/` のサブモジュール単位に分割する作業** → 別 issue。本 issue は dir 化 (1 ファイル分の `main.rs` に rename) のみ実施。CLAUDE.md L88 の「サブモジュール対応で分割」を完全達成するのは中身分割 issue で行う
- `prop_connection/main.rs` の今後のサブモジュール対応 → `src/connection/` は現状 `mod.rs` のみで分割対象がなく、`main.rs` 単独で規約準拠を維持できる

## テスト戦略

本 issue はファイル名と配置の rename のみ。テストロジック・strategy・assert は無変更。

- **件数一致**: `cargo test --workspace` の合計 passed 件数が移行前後で完全一致。CLAUDE.md L85 で `#[ignore]` 禁止のため `ignored` カウントは常に 0、`passed` だけ比較すれば十分
- **カバレッジ低下なし**: `cargo llvm-cov report` で PBT 経路の行カバレッジが同等以上
- **`pbt/Cargo.toml` 影響確認**: Cargo auto-discovery で `prop_hpack/main.rs` 等が integration test として認識されることを `cargo test --no-run` 後の `target/debug/deps/` の test バイナリ列挙で確認

## RFC 引用

本 issue は PBT ファイルの命名・配置の整理のみで RFC 引用は不要。

## 依存

Phase ごとに blocking 関係が異なるため Phase 別に列挙する。

- **Phase A**: [[0034-refactor-consolidate-field-syntax-module]] (blocking。`src/syntax.rs` 新設後にのみ `prop_syntax.rs` rename 可能)
- **Phase B / C / D**: blocking 依存なし (0034 と独立に着手可能。ディレクトリ化と既存ファイルの rename のみ)
- 関連: [[0036-refactor-move-mod-tests-to-tests-dir]] (`tests/` 側の規約準拠と PBT 側の規約準拠は CLAUDE.md で対応関係にあり、両 issue 完了で全テスト命名が規約準拠する)
- 関連: [[0038-add-pbt-for-cookie-and-empty-path]] (本 issue 完了後の `prop_connection/main.rs` 構成に新規 PBT が追加される場合に整合させる)

## 解決方法

PBT ファイルの命名と配置を CLAUDE.md 規約 (`pbt/tests/prop_<module>.rs` / dir module 用 `prop_<module>/main.rs`) に整合させた。

### Phase A: prop_header_field_syntax.rs → prop_syntax.rs
0045 で `prop_header_field_syntax.rs` は crate 内 `#[cfg(test)]` に移管済みのため、本 issue では物理ファイル削除済み (作業不要)。

### Phase B: prop_hpack/ ディレクトリ化
- `pbt/tests/prop_hpack.rs` → `pbt/tests/prop_hpack/main.rs` (git mv)
- `pbt/tests/prop_dynamic_table.rs` → `pbt/tests/prop_hpack/dynamic_table.rs` (git mv)
- `prop_hpack/main.rs` に `mod dynamic_table;` 宣言を追加

### Phase C: prop_stream/ ディレクトリ化
- `pbt/tests/prop_stream_state.rs` → `pbt/tests/prop_stream/state.rs` (git mv)
- `pbt/tests/prop_stream/main.rs` を新規作成 (`mod state;` のみのスケルトン)

### Phase D: prop_frame/ と prop_webtransport/ ディレクトリ化
- `pbt/tests/prop_frame.rs` → `pbt/tests/prop_frame/main.rs` (git mv)
- `pbt/tests/prop_webtransport.rs` → `pbt/tests/prop_webtransport/main.rs` (git mv)

### その他
- `prop_flow_control.rs` 先頭に名前衝突方針の doc コメントを追加
- ファイル名を `0039-fix-pbt-naming-convention.md` → `0039-refactor-pbt-naming-convention.md` に変更 (category を fix から refactor に修正)
- 3 つの closed issue 内の wikilink を更新
