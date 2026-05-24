#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{HeaderField, HpackDecoder, HpackEncoder};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    table_size: u16,
    use_huffman: bool,
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
    let table_size = input.table_size as usize;

    // 任意バイトを wire ヘルパで HeaderField に変換する (HPACK decoder 経路)。
    // 目的: HPACK encoder / decoder が wire 上の不正データに対しても panic
    // しないこと、および valid に構築できたデータは roundtrip すること。
    let headers: Vec<HeaderField> = input
        .headers
        .iter()
        .map(|h| wire_header_field(&h.name, &h.value))
        .collect();

    if headers.is_empty() {
        return;
    }

    // エンコード
    let mut encoder = HpackEncoder::new(table_size);
    encoder.set_huffman(input.use_huffman);
    let mut buf = Vec::new();
    encoder.encode(&mut buf, &headers);

    // デコード
    let mut decoder = HpackDecoder::new(table_size);
    if let Ok(decoded) = decoder.decode(&buf) {
        // ラウンドトリップの一致を検証する
        assert_eq!(headers.len(), decoded.len(), "header count mismatch");
        for (original, decoded) in headers.iter().zip(decoded.iter()) {
            assert_eq!(original.name(), decoded.name(), "header name mismatch");
            assert_eq!(original.value(), decoded.value(), "header value mismatch");
        }
    }
    // デコード失敗自体は wire 不正データなので許容する (panic しないことが目的)。
});
