#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::webtransport::varint;

fuzz_target!(|data: &[u8]| {
    // 任意のバイト列を varint としてデコード
    let _ = varint::decode(data);
});
