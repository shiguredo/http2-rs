#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{
    Connection, ErrorCode, HeaderField, HpackDecoder, Limits, Role, StreamId,
};

#[derive(Debug, Arbitrary)]
enum FuzzAction {
    Feed(Vec<u8>),
    StartStream {
        headers: Vec<FuzzHeader>,
        end_stream: bool,
    },
    SendResponse {
        stream_id: u32,
        headers: Vec<FuzzHeader>,
        end_stream: bool,
    },
    SendData {
        stream_id: u32,
        data: Vec<u8>,
        end_stream: bool,
    },
    SendTrailers {
        stream_id: u32,
        headers: Vec<FuzzHeader>,
    },
    ResetStream {
        stream_id: u32,
        error_code: u32,
    },
    SendPing {
        opaque_data: [u8; 8],
    },
    SendGoaway {
        error_code: u32,
        debug_data: Vec<u8>,
    },
    SendWindowUpdate {
        stream_id: u32,
        increment: u32,
    },
    Process,
    PollEvent,
    PollOutput,
}

#[derive(Debug, Arbitrary)]
struct FuzzHeader {
    name: Vec<u8>,
    value: Vec<u8>,
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    role_is_server: bool,
    actions: Vec<FuzzAction>,
}

/// HPACK string literal (RFC 7541 §5.2) を符号化する。
/// H=0 (Huffman off)、String Length は 7-bit prefix 整数 (§5.1) で符号化。
fn encode_string(buf: &mut Vec<u8>, data: &[u8]) {
    let mut temp = [0u8; 16];
    let len = shiguredo_http2::hpack::integer::encode(
        &mut temp,
        data.len() as u64,
        7,
        0x00,
    )
    .expect("infallible: 16 bytes exceeds HPACK integer maximum of 11 bytes");
    buf.extend_from_slice(&temp[..len]);
    buf.extend_from_slice(data);
}

/// 検査なしの name/value を HPACK Literal Header Field without Indexing
/// (RFC 7541 §6.2.2) として符号化し、HpackDecoder でデコードして
/// HeaderField を返す (wire 模擬)。
fn wire_header_field(name: &[u8], value: &[u8]) -> HeaderField {
    let mut wire = Vec::new();
    wire.push(0x00);
    encode_string(&mut wire, name);
    encode_string(&mut wire, value);
    let mut decoder = HpackDecoder::new(0);
    let headers = decoder
        .decode(&wire)
        .expect("infallible: wire_header_field produced invalid HPACK");
    headers
        .into_iter()
        .next()
        .expect("infallible: wire encoding produces exactly one header")
}

fuzz_target!(|input: FuzzInput| {
    // ローカル操作とリモート入力を任意に交互実行する。
    // 複合状態遷移でのパニック安全性を検証する。
    let limits = Limits::default();
    let mut conn = if input.role_is_server {
        let mut c = Connection::new(Role::Server, limits);
        c.mark_preface_received();
        c
    } else {
        Connection::new(Role::Client, limits)
    };

    let _ = conn.initiate();
    while conn.poll_output().is_some() {}

    // actions は先頭 256 件に切り詰める (fuzzer のスループット確保のため)
    for action in input.actions.iter().take(256) {
        match action {
            FuzzAction::Feed(data) => {
                let _ = conn.feed(data);
            }
            FuzzAction::StartStream {
                headers,
                end_stream,
            } => {
                let hdr: Vec<HeaderField> = headers
                    .iter()
                    .map(|h| wire_header_field(&h.name, &h.value))
                    .collect();
                let _ = conn.start_stream(hdr, *end_stream);
            }
            FuzzAction::SendResponse {
                stream_id,
                headers,
                end_stream,
            } => {
                let hdr: Vec<HeaderField> = headers
                    .iter()
                    .map(|h| wire_header_field(&h.name, &h.value))
                    .collect();
                let sid = StreamId::from_wire(*stream_id & 0x7FFF_FFFF);
                let _ = conn.send_response(sid, hdr, *end_stream);
            }
            FuzzAction::SendData {
                stream_id,
                data,
                end_stream,
            } => {
                let sid = StreamId::from_wire(*stream_id & 0x7FFF_FFFF);
                let _ = conn.send_data(sid, data.clone(), *end_stream);
            }
            FuzzAction::SendTrailers {
                stream_id,
                headers,
            } => {
                let hdr: Vec<HeaderField> = headers
                    .iter()
                    .map(|h| wire_header_field(&h.name, &h.value))
                    .collect();
                let sid = StreamId::from_wire(*stream_id & 0x7FFF_FFFF);
                let _ = conn.send_trailers(sid, hdr);
            }
            FuzzAction::ResetStream {
                stream_id,
                error_code,
            } => {
                let sid = StreamId::from_wire(*stream_id & 0x7FFF_FFFF);
                let ec = ErrorCode::from_u32(*error_code);
                let _ = conn.reset_stream(sid, ec);
            }
            FuzzAction::SendPing { opaque_data } => {
                let _ = conn.send_ping(*opaque_data);
            }
            FuzzAction::SendGoaway {
                error_code,
                debug_data,
            } => {
                let ec = ErrorCode::from_u32(*error_code);
                let _ = conn.send_goaway(ec, debug_data.clone());
            }
            FuzzAction::SendWindowUpdate {
                stream_id,
                increment,
            } => {
                let sid = StreamId::from_wire(*stream_id & 0x7FFF_FFFF);
                let _ = conn.send_window_update(sid, *increment);
            }
            FuzzAction::Process => {
                let _ = conn.process();
            }
            FuzzAction::PollEvent => {
                let _ = conn.poll_event();
            }
            FuzzAction::PollOutput => {
                let _ = conn.poll_output();
            }
        }
    }
});
