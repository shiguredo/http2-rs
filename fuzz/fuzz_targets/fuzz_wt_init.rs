#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::webtransport::WtInit;

fuzz_target!(|data: &[u8]| {
    // 任意のバイト列でパニックしないことを検証する
    let _ = WtInit::parse(data);
});
