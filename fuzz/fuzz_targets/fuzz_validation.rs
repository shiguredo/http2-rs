#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::HeaderField;
use shiguredo_http2::validation::{
    validate_request_headers, validate_response_headers, validate_trailers,
};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    headers: Vec<FuzzHeader>,
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
    let mut decoder = shiguredo_http2::HpackDecoder::new(0);
    let headers = decoder
        .decode(&wire)
        .expect("infallible: wire_header_field produced invalid HPACK");
    headers
        .into_iter()
        .next()
        .expect("infallible: wire encoding produces exactly one header")
}

fuzz_target!(|input: FuzzInput| {
    // 任意バイトを wire ヘルパで HeaderField に変換する (HPACK decoder 経路)。
    // 目的: validation 層が wire 上の不正データに対しても panic しないこと。
    let headers: Vec<HeaderField> = input
        .headers
        .iter()
        .map(|h| wire_header_field(&h.name, &h.value))
        .collect();

    let _ = validate_request_headers(&headers);
    let _ = validate_response_headers(&headers);
    let _ = validate_trailers(&headers);
});
