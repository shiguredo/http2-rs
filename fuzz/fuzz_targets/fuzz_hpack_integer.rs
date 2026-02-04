#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::hpack::integer;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // 先頭 1 バイトから prefix_bits を決定する (1-8)
    let prefix_bits = (data[0] % 8) + 1;
    let decode_data = &data[1..];

    // 任意のバイト列を HPACK 整数としてデコードする
    // オーバーフローや不完全入力の処理を検証する
    let _ = integer::decode(decode_data, prefix_bits);
});
