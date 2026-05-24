# PBT ディレクトリモジュールをサブモジュール単位に分割する

Created: 2026-05-25
Priority: Low
Model: deepseek-v4-pro

## 背景

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

## 修正方針

1. `prop_frame/main.rs` 内の proptest ブロックを、対応する `src/frame/` サブモジュールごとに分類する
2. 各サブモジュールファイルに移動し、`main.rs` に `mod` 宣言を追加する
3. `prop_webtransport/main.rs` も同様に分割する
4. 共通の import や helper は `main.rs` に残すか、各サブモジュールで個別に import する

## 受け入れ基準

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
