#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::FlowControl;

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

fuzz_target!(|input: FuzzInput| {
    // FlowControl に対して全ての公開メソッドを任意順序で呼び出し、
    // パニック安全性とオーバーフロー/アンダーフロー耐性を検証する。
    let mut fc = FlowControl::new(input.initial_window_size);
    for action in input.actions.iter().take(256) {
        match action {
            FlowAction::ConsumeSend(v) => {
                let _ = fc.consume_send(*v as usize);
            }
            FlowAction::ConsumeRecv(v) => {
                let _ = fc.consume_recv(*v as usize);
            }
            FlowAction::RecvWindowUpdate(v) => {
                let _ = fc.recv_window_update(*v);
            }
            FlowAction::AddRecvWindow(v) => {
                let _ = fc.add_recv_window(*v);
            }
            FlowAction::UpdateInitialWindowSize(v) => {
                let _ = fc.update_initial_window_size(*v);
            }
        }
    }
});
