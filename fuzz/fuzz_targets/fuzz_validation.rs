#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::validation::{validate_request_headers, validate_response_headers, validate_trailers};
use shiguredo_http2::HeaderField;

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    headers: Vec<FuzzHeader>,
}

#[derive(Debug, Arbitrary)]
struct FuzzHeader {
    name: Vec<u8>,
    value: Vec<u8>,
}

fuzz_target!(|input: FuzzInput| {
    let headers: Vec<HeaderField> = input
        .headers
        .iter()
        .map(|h| HeaderField::new(h.name.clone(), h.value.clone()))
        .collect();

    // リクエストヘッダー検証
    let _ = validate_request_headers(&headers);

    // レスポンスヘッダー検証
    let _ = validate_response_headers(&headers);

    // トレーラー検証
    let _ = validate_trailers(&headers);
});
