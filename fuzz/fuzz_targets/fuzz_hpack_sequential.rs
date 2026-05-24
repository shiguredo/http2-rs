#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{HeaderField, HpackDecoder, HpackEncoder};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_table_size: u16,
    rounds: Vec<FuzzRound>,
}

#[derive(Debug, Arbitrary)]
struct FuzzRound {
    headers: Vec<FuzzHeader>,
    new_table_size: Option<u16>,
}

#[derive(Debug, Arbitrary)]
struct FuzzHeader {
    name: Vec<u8>,
    value: Vec<u8>,
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
    // 複数ラウンドのヘッダーリストを連続でエンコード/デコードし、
    // 動的テーブルの状態遷移に対するパニック安全性を検証する。
    let table_size = input.initial_table_size as usize;
    let mut encoder = HpackEncoder::new(table_size);
    let mut decoder = HpackDecoder::new(table_size);

    // rounds は先頭 64 件に切り詰める (fuzzer のスループット確保のため)
    for round in input.rounds.iter().take(64) {
        let mut buf = Vec::new();

        // テーブルサイズ変更 (RFC 7541 §4.2)
        if let Some(new_size) = round.new_table_size {
            let new_size = new_size as usize;
            encoder.set_max_table_size(new_size);
            encoder.encode_size_update(&mut buf, new_size);
            decoder.set_max_table_size(new_size);
        }

        let headers: Vec<HeaderField> = round
            .headers
            .iter()
            .map(|h| wire_header_field(&h.name, &h.value))
            .collect();
        encoder.encode(&mut buf, &headers);
        let _ = decoder.decode(&buf);
    }
});
