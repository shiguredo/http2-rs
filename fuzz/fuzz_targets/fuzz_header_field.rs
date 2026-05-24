#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::HeaderField;

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    name: Vec<u8>,
    value: Vec<u8>,
    sensitive: bool,
}

fuzz_target!(|input: FuzzInput| {
    // 任意の name/value バイト列を HeaderField に渡し、パニック安全性のみを検証する。
    let _ = HeaderField::new(&input.name, &input.value);
    let _ = HeaderField::new_with_sensitive(&input.name, &input.value, input.sensitive);
});
