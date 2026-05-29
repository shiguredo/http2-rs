# PBT ディレクトリモジュールをサブモジュール単位に分割する

- Priority: Low
- Created: 2026-05-25
- Completed: 2026-05-29
- Model: deepseek-v4-pro

## 目的

issue 0039 で PBT ファイルをディレクトリモジュール形式 (`prop_<module>/main.rs`) に移行した。しかし 0039 のスコープは「既存ファイル全体を `main.rs` に rename するのみ」であり、`src/frame/` / `src/webtransport/` 配下のサブモジュールごとの分割は本 issue で行う。

AGENTS.md: 「`src/<module>/` のようにディレクトリモジュールの場合は `pbt/tests/prop_<module>/main.rs` にサブモジュール対応で分割すること」

## 対象

### `pbt/tests/prop_frame/main.rs` (1,644 行)

`src/frame/` のサブモジュール構成:
- `src/frame/mod.rs`
- `src/frame/decoder.rs`
- `src/frame/encoder.rs`
- `src/frame/error.rs`
- `src/frame/flags.rs`

分割先:
```
pbt/tests/prop_frame/
├── main.rs          — mod 宣言 + フレーム共通の PBT
├── decoder.rs       — decoder 関連 PBT
├── encoder.rs       — encoder 関連 PBT
└── (必要に応じて追加)
```

### `pbt/tests/prop_webtransport/main.rs` (717 行)

`src/webtransport/` のサブモジュール構成:
- `src/webtransport/mod.rs`
- `src/webtransport/stream.rs`
- `src/webtransport/capsule.rs`
- `src/webtransport/flow_control.rs`
- `src/webtransport/varint.rs`

分割先:
```
pbt/tests/prop_webtransport/
├── main.rs          — mod 宣言 + セッションレベル PBT
├── capsule.rs       — capsule 関連 PBT
├── flow_control.rs  — WebTransport フロー制御 PBT
├── varint.rs        — varint 関連 PBT
└── (必要に応じて追加)
```

## 設計方針

1. `prop_frame/main.rs` 内の proptest ブロックを、対応する `src/frame/` サブモジュールごとに分類する
2. 各サブモジュールファイルに移動し、`main.rs` に `mod` 宣言を追加する
3. `prop_webtransport/main.rs` も同様に分割する
4. 共通の import や helper は `main.rs` に残すか、各サブモジュールで個別に import する

## 完了条件

- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
- 移行前後で `cargo test` の passed 件数が一致する
- `prop_frame/main.rs` と `prop_webtransport/main.rs` がサブモジュール宣言 + 共通テストのみになっている

## CHANGES.md (実装時に追記)

- `## develop` の `### misc` に以下を追加する:
  - `[UPDATE]` PBT ディレクトリモジュールを `src/` のサブモジュール構成に対応して分割する
    - @voluntas

## 依存

- 0039 (完了済み): ディレクトリモジュール形式への移行が前提

## 対応不要と判断した理由

本 issue は対応せず close する。理由は以下のとおり。

- `src/frame/` は処理フェーズ軸 (`decoder.rs` / `encoder.rs`) で分割されているが、`prop_frame` の PBT は機能軸 (フレーム種別) と往復テスト中心で書かれており、両者の軸が直交する。そのため CLAUDE.md「`src/<module>/` のようにディレクトリモジュールの場合は `pbt/tests/prop_<module>/main.rs` にサブモジュール対応で分割すること」を `prop_frame` に適用すると、encoder と decoder の両方を同時に exercise する往復テスト約 25 件がどのサブモジュールにも一意に割り当てられず、規約どおりの分割が原理的に成立しない。
- `prop_frame` 末尾の `from_static_consistency` は `ClientStreamId` / `ServerStreamId` / `NonZeroStreamId` (いずれも `src/stream_id.rs` 由来) と `WindowIncrement` / `Weight` / `LastStreamId` (`src/frame/error.rs` 由来) が混在しており、`src/frame/` のサブモジュールに対応しない。分割を完結させるには `prop_stream_id.rs` の新設まで踏み込む必要があり、本 issue の主題から外れる。
- 本 issue が提案する `prop_webtransport/flow_control.rs` には対応する PBT が存在しない。`WtFlowControl` (`src/webtransport/flow_control.rs`) を直接検証する PBT は未作成で、フロー制御テスト (`prop_wt_stream_*_flow_control`) は `WtStream` (`src/webtransport/stream.rs`) を検証している。これは 0039 が将来別 issue としたスコープであり、本 issue の分割対象ではない。
- `prop_webtransport` (717 行) は varint / capsule / stream / session に素直に分割できるが、ファイルサイズが逼迫しておらず Priority も Low のため、今分割する必然性は低い。
- 将来 `prop_webtransport` の肥大化や `WtFlowControl` の PBT 追加が必要になった時点で、webtransport に限定した issue を改めて作成すればよい。
