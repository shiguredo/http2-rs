#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::hpack::huffman;

fuzz_target!(|data: &[u8]| {
    // 任意のバイト列を Huffman デコードする
    // パディング異常やデコードテーブルの境界値を検証する
    let _ = huffman::decode(data);
});
