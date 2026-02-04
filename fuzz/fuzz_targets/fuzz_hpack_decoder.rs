#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::HpackDecoder;

fuzz_target!(|data: &[u8]| {
    let mut decoder = HpackDecoder::new(4096);

    // 任意のバイト列をヘッダーブロックとしてデコード
    let _ = decoder.decode(data);
});
