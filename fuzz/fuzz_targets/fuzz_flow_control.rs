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
    ShouldSendWindowUpdate,
    WindowUpdateIncrement,
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    send_initial_window_size: u32,
    recv_initial_window_size: u32,
    actions: Vec<FlowAction>,
}

fuzz_target!(|input: FuzzInput| {
    // FlowControl::with_separate_windows で送信側と受信側の初期ウィンドウサイズを
    // 独立に設定し、非対称な初期状態から各メソッドを任意順序で呼び出して
    // パニック安全性 (オーバーフロー/アンダーフロー耐性を含む) を検証する。
    // 戻り値の正しさは PBT (pbt/tests/prop_flow_control.rs) が担保するため、
    // ここでは値の検証は行わない。
    let mut fc = FlowControl::with_separate_windows(
        input.send_initial_window_size,
        input.recv_initial_window_size,
    );
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
            FlowAction::ShouldSendWindowUpdate => {
                let _ = fc.should_send_window_update();
            }
            FlowAction::WindowUpdateIncrement => {
                let _ = fc.window_update_increment();
            }
        }
    }
});
