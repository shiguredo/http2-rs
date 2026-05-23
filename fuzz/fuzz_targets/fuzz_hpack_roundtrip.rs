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

fuzz_target!(|input: FuzzInput| {
    let table_size = input.table_size as usize;

    // 任意バイトを HeaderField として直接構築する (構築時検査をバイパス)。
    // 目的: HPACK encoder / decoder が wire 上の不正データに対しても panic
    // しないこと、および valid に構築できたデータは roundtrip すること。
    let headers: Vec<HeaderField> = input
        .headers
        .iter()
        .map(|h| HeaderField::from_validated_parts(h.name.clone(), h.value.clone(), false))
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
