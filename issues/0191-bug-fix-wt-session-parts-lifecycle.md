# `WtSessionParts` の driver タスクが終了せず、Drop の doc も実装と異なる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-session-parts-lifecycle
- Polished: {YYYY-MM-DD}

## 目的

`WtSessionParts` を `WtServerSession::into_parts` で取り出した後、利用者が `WtSessionParts` を drop しても driver タスクと TCP 接続が残る。`DriverState` が `cmd_tx` を自己保持するため、外部の sender がすべて drop されても `cmd_rx.recv()` が `None` にならない。さらに `WtSessionParts::driver` の doc は「drop で abort される」と書くが、`JoinHandle` の drop は detach であり abort ではない。タスクと接続のライフサイクルを明確にする。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `accept()` は `cmd_tx.clone()` を `DriverState` に渡す。`DriverState` は新しいストリームへ `cmd_tx` を clone して渡すため、自身が sender を保持し続ける。

`WtServerSession` は Drop で `driver.abort()` するが、`WtSessionParts` には Drop がなく、`driver` の `JoinHandle` を drop しても abort されない。`WtServerSession` の doc は「drop 時は cmd_tx が閉じ、driver が自然に終了する」と書くが、実装は abort である。

## 設計方針

- `DriverState` の自己 sender を外し、ストリーム生成時に外部 sender を clone して渡す。
- `WtSessionParts` に Drop を実装して driver を abort する、または明示的な shutdown API を追加する。
- `WtServerSession` / `WtSessionParts` / `driver` の doc を実装に合わせる。
- `WtSessionParts` を drop したときに driver タスクと接続が終了することを検証するテストを追加する。

## 完了条件

- `WtSessionParts` を drop すると driver タスクが終了し、接続が残らないこと。
- `cmd_tx` の自己保持が解消されていること。
- `WtServerSession` / `WtSessionParts` / `driver` の doc が実装と一致していること。
- ライフサイクルのテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
