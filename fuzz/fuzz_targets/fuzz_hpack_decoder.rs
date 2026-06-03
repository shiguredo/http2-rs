#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{HeaderField, HpackDecoder};

fuzz_target!(|data: &[u8]| {
    // 上限なし: 任意のバイト列でパニックしないこと
    let mut decoder = HpackDecoder::new(4096);
    let _ = decoder.decode(data);

    // 上限あり: デコードが成功した場合、デコード後ヘッダーリストサイズが
    // 必ず上限以下であること (RFC 9113 Section 6.5.2)。
    // インデックス参照爆弾に対する逐次上限の回帰を検知する。
    let limit = 16384usize;
    let mut bounded = HpackDecoder::new(4096);
    bounded.set_max_header_list_size(Some(limit));
    if let Ok(headers) = bounded.decode(data) {
        let size: usize = headers.iter().map(HeaderField::size).sum();
        assert!(
            size <= limit,
            "decoded header list size {} exceeds limit {}",
            size,
            limit
        );
    }
});
