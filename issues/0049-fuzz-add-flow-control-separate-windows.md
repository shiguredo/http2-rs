# fuzz_flow_control に with_separate_windows 経路を追加する

- Priority: Low
- Created: 2026-05-26
- Model: deepseek-v4-pro
- Branch: feature/add-fuzz-flow-control-separate-windows

## 目的

`fuzz/fuzz_targets/fuzz_flow_control.rs` は現在 `FlowControl::new(initial_window_size)` のみで `FlowControl` を構築しており、`send_initial != recv_initial` となる `FlowControl::with_separate_windows` 経路がカバーされていない。issue 0014 (`issues/closed/0014-bug-fix-flow-control-critical.md`) の完了時 TODO として起票された issue である。issue 0014 で `with_separate_windows` のバグを修正した際に、この経路のパニック安全性が fuzz で検証されていないことが判明した。

## 優先度根拠

issue 0014 で修正されたバグは `with_separate_windows` 固有の経路であり、今後同様の回帰バグが入った場合に fuzz で検出できるようにしておくべきである。ただし `pbt/tests/prop_flow_control.rs` の PBT（`prop_recv_methods_independent_of_send_initial`, `prop_update_initial_preserves_recv_initial`, `prop_recv_window_above_initial`）で主要なプロパティは検証済みであり、直近のリスクは低いため Low とする。

## 現状

`fuzz/fuzz_targets/fuzz_flow_control.rs` の `FuzzInput` 構造体:

```rust
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_window_size: u32,
    actions: Vec<FlowAction>,
}
```

`FlowControl::new(input.initial_window_size)` で生成するため、常に `send_initial == recv_initial` となる。`should_send_window_update()` と `window_update_increment()` が `recv_initial` を基準に判定する修正後のロジックは、`send_initial != recv_initial` のケースでのみ差が顕在化するが、fuzz ではそのケースが生成されない。

## 設計方針

`FuzzInput` に `recv_initial_window_size: u32` フィールドを追加し、`FlowControl::with_separate_windows` で構築する。`FlowAction` に `ShouldSendWindowUpdate` と `WindowUpdateIncrement` を追加する。これらのメソッド自体は現時点でパニック経路を持たないが、内部状態（`recv_window`, `recv_initial`）に依存する演算を含むため、将来の実装変更で整数演算の前提が崩れた場合の回帰検出に有効である。コストは enum バリアント 2 つの追加のみで、fuzz のスループットへの影響は無視できる。

```rust
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    send_initial_window_size: u32,
    recv_initial_window_size: u32,
    actions: Vec<FlowAction>,
}

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
```

fuzz target 本体:

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

## 完了条件

- `FuzzInput` が `send_initial_window_size` と `recv_initial_window_size` の 2 フィールドを持つ
- `FlowControl::with_separate_windows` で構築している
- `FlowAction` に `ShouldSendWindowUpdate` と `WindowUpdateIncrement` が追加されている
- `cargo check --manifest-path fuzz/Cargo.toml` が通る
- `cargo clippy --manifest-path fuzz/Cargo.toml -- -D warnings` が通る（fuzz クレートはワークスペースから除外されているため個別指定）
- `cargo test --workspace` が通る（公開 API 変更がないことの確認）
