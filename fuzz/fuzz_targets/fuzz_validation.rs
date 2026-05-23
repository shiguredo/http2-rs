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

fuzz_target!(|input: FuzzInput| {
    // 任意バイトを HeaderField として直接構築する (構築時検査をバイパス)。
    // 目的: validation 層が wire 上の不正データに対しても panic しないこと。
    let headers: Vec<HeaderField> = input
        .headers
        .iter()
        .map(|h| HeaderField::from_validated_parts(h.name.clone(), h.value.clone(), false))
        .collect();

    let _ = validate_request_headers(&headers);
    let _ = validate_response_headers(&headers);
    let _ = validate_trailers(&headers);
});
