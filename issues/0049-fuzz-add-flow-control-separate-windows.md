# fuzz_flow_control に with_separate_windows 経路を追加する

- Priority: Low
- Created: 2026-05-26
- Polished: 2026-05-29
- Model: deepseek-v4-pro
- Branch: feature/add-fuzz-flow-control-separate-windows

## 目的

`fuzz/fuzz_targets/fuzz_flow_control.rs` は現在 `FlowControl::new(initial_window_size)` のみで `FlowControl` を構築しており、`FlowControl::with_separate_windows` 経路がカバーされていない。issue 0014 (`issues/closed/0014-bug-fix-flow-control-critical.md`) でこの経路のバグを修正した際に、fuzz で未検証であることが判明したため起票した。

## 優先度根拠

issue 0014 で修正したバグは `with_separate_windows` 固有の経路であり、同様の回帰バグが将来入った場合に fuzz でパニックを検出できるようにしておくべきである。ただし主要なプロパティは `pbt/tests/prop_flow_control.rs` の PBT で検証済みであり、直近のリスクは低いため Low とする。

## 現状

`fuzz/fuzz_targets/fuzz_flow_control.rs` の現状の定義:

```rust
#[derive(Debug, Arbitrary)]
enum FlowAction {
    ConsumeSend(u32),
    ConsumeRecv(u32),
    RecvWindowUpdate(u32),
    AddRecvWindow(u32),
    UpdateInitialWindowSize(u32),
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_window_size: u32,
    actions: Vec<FlowAction>,
}
```

`FlowControl::new(input.initial_window_size)` で生成するため、`send_initial` と `recv_initial` が常に同値になる。`should_send_window_update()` / `window_update_increment()` は `recv_initial` のみを基準に判定する (`src/flow_control.rs:186-201`) が、`recv_initial` が `send_initial` と独立な値を取る状態（`with_separate_windows` でのみ生じる）が fuzz で一切生成されない。

## 設計方針

`FuzzInput` の `initial_window_size` を `send_initial_window_size` と `recv_initial_window_size` の 2 フィールドに分割し、`FlowControl::with_separate_windows` で構築する。これにより、`new()` では送信側と受信側で独立に振れなかった初期状態を fuzzer が探索できるようになり、非対称な初期状態から全ての `&mut self` メソッドを呼ぶ経路をカバーできる。

`FlowAction` に `ShouldSendWindowUpdate` と `WindowUpdateIncrement` を追加する。この 2 つは issue 0014 で修正したメソッドであり、`recv_initial` を基準に除算・減算・キャストを行う (`src/flow_control.rs:186-201`)。fuzz 対象に含めることで、将来の実装変更でこれらの整数演算にパニック経路が入り込んだ場合に検出できる。

fuzz target は戻り値を全て破棄しており、検証するのはパニック安全性のみである。これらメソッドの戻り値の正しさは PBT (`prop_recv_methods_independent_of_send_initial`) が担保しており、issue 0014 で追加済みである。本 issue で fuzz に値の検証 (assert) を持ち込まない。

純粋な getter (`send_window` / `recv_window` / `send_initial` / `recv_initial`) はフィールドをそのまま返すだけでパニック経路がないため fuzz 対象に含めない。`send_available()` も `send_window`（送信側）のみに依存し `with_separate_windows` で新たな経路が生じないため対象外とする。

```rust
#[derive(Debug, Arbitrary)]
enum FlowAction {
    ConsumeSend(u32),
    ConsumeRecv(u32),
    RecvWindowUpdate(u32),
    AddRecvWindow(u32),
    UpdateInitialWindowSize(u32),
    ShouldSendWindowUpdate,
    WindowUpdateIncrement,
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    send_initial_window_size: u32,
    recv_initial_window_size: u32,
    actions: Vec<FlowAction>,
}
```

fuzz target 本体（`take(256)` の上限は手本 `fuzz_wt_flow_control.rs` に倣う）:

```rust
fuzz_target!(|input: FuzzInput| {
    let mut fc = FlowControl::with_separate_windows(
        input.send_initial_window_size,
        input.recv_initial_window_size,
    );
    for action in input.actions.iter().take(256) {
        match action {
            FlowAction::ConsumeSend(v) => { let _ = fc.consume_send(*v as usize); }
            FlowAction::ConsumeRecv(v) => { let _ = fc.consume_recv(*v as usize); }
            FlowAction::RecvWindowUpdate(v) => { let _ = fc.recv_window_update(*v); }
            FlowAction::AddRecvWindow(v) => { let _ = fc.add_recv_window(*v); }
            FlowAction::UpdateInitialWindowSize(v) => { let _ = fc.update_initial_window_size(*v); }
            FlowAction::ShouldSendWindowUpdate => { let _ = fc.should_send_window_update(); }
            FlowAction::WindowUpdateIncrement => { let _ = fc.window_update_increment(); }
        }
    }
});
```

### 変更対象ファイル

- `fuzz/fuzz_targets/fuzz_flow_control.rs`: 上記の通り修正

`src/` は変更しない。fuzz target はライブラリの公開 API・挙動を変えないため CHANGES.md への追記は不要（`fuzz_flow_control` 自体は issue 0046 の `[ADD]` エントリで既に記載済みであり、本 issue は既存 target の内部拡張に過ぎない）。

## 完了条件

- `FuzzInput` が `send_initial_window_size` と `recv_initial_window_size` の 2 フィールドを持つ
- `FlowControl::with_separate_windows` で構築している
- `FlowAction` に `ShouldSendWindowUpdate` と `WindowUpdateIncrement` が追加されている
- `cargo check --manifest-path fuzz/Cargo.toml` が通る
- `cargo clippy --manifest-path fuzz/Cargo.toml -- -D warnings` が通る（fuzz クレートはワークスペースから除外されているため個別指定）
- `cargo test --workspace` が通る

`cargo fuzz run fuzz_flow_control` による実コーパス実行は、実行時間が不定のため完了条件に含めない（fuzz target のコンパイル可能性検証に留める前例: issue 0037）。
