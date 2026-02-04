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

    let headers: Vec<HeaderField> = input
        .headers
        .iter()
        .map(|h| HeaderField::new(h.name.clone(), h.value.clone()))
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
    match decoder.decode(&buf) {
        Ok(decoded) => {
            // ラウンドトリップの一致を検証する
            assert_eq!(
                headers.len(),
                decoded.len(),
                "ヘッダー数が一致しない"
            );
            for (original, decoded) in headers.iter().zip(decoded.iter()) {
                assert_eq!(
                    original.name, decoded.name,
                    "ヘッダー名が一致しない"
                );
                assert_eq!(
                    original.value, decoded.value,
                    "ヘッダー値が一致しない"
                );
            }
        }
        Err(_) => {
            // エンコードしたデータがデコードできない場合はバグ
            panic!("エンコードしたデータのデコードに失敗した");
        }
    }
});
