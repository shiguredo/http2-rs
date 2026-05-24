// Property-Based Testing library crate

use shiguredo_http2::{HeaderField, HpackDecoder};

/// 検査なしの name/value を HPACK Literal Header Field without Indexing
/// (RFC 7541 §6.2.2) として符号化し、HpackDecoder でデコードして
/// HeaderField を返す (wire 模擬)。
pub fn wire_header_field(name: &[u8], value: &[u8]) -> HeaderField {
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

/// HPACK string literal (RFC 7541 §5.2) を符号化する。
/// H=0 (Huffman off)、String Length は 7-bit prefix 整数 (§5.1) で符号化。
/// 16 バイトバッファは 7-bit prefix 整数の最大長 (u64 で 11 バイト) に十分。
fn encode_string(buf: &mut Vec<u8>, data: &[u8]) {
    let mut temp = [0u8; 16];
    let len = shiguredo_http2::hpack::integer::encode(&mut temp, data.len() as u64, 7, 0x00)
        .expect("infallible: 16 bytes exceeds HPACK integer maximum of 11 bytes");
    buf.extend_from_slice(&temp[..len]);
    buf.extend_from_slice(data);
}
