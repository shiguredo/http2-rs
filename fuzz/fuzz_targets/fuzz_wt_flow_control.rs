#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::webtransport::WtFlowControl;

#[derive(Debug, Arbitrary)]
enum WtFlowAction {
    ConsumeSend(u64),
    ConsumeRecv(u64),
    UpdateSendMax(u64),
    AddRecvMax(u64),
    UpdateMaxStreams {
        maximum: u64,
        bidirectional: bool,
    },
    AddMaxStreamsLocal {
        increment: u64,
        bidirectional: bool,
    },
    OpenedStream {
        bidirectional: bool,
    },
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_max_data: u64,
    max_streams_bidi: u64,
    max_streams_uni: u64,
    actions: Vec<WtFlowAction>,
}

fuzz_target!(|input: FuzzInput| {
    // WtFlowControl に対して全ての公開メソッドを任意順序で呼び出し、
    // パニック安全性を検証する。
    let mut fc = WtFlowControl::new(
        input.initial_max_data,
        input.initial_max_data,
        input.max_streams_bidi,
        input.max_streams_bidi,
        input.max_streams_uni,
        input.max_streams_uni,
    );
    for action in input.actions.iter().take(256) {
        match action {
            WtFlowAction::ConsumeSend(v) => {
                let _ = fc.consume_send(*v);
            }
            WtFlowAction::ConsumeRecv(v) => {
                let _ = fc.consume_recv(*v);
            }
            WtFlowAction::UpdateSendMax(v) => {
                let _ = fc.update_send_max(*v);
            }
            WtFlowAction::AddRecvMax(v) => {
                let _ = fc.add_recv_max(*v);
            }
            WtFlowAction::UpdateMaxStreams {
                maximum,
                bidirectional,
            } => {
                let _ = fc.update_max_streams(*maximum, *bidirectional);
            }
            WtFlowAction::AddMaxStreamsLocal {
                increment,
                bidirectional,
            } => {
                fc.add_max_streams_local(*increment, *bidirectional);
            }
            WtFlowAction::OpenedStream { bidirectional } => {
                fc.opened_stream(*bidirectional);
            }
        }
    }
});
