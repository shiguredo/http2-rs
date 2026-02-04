#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::webtransport::CapsuleDecoder;

fuzz_target!(|data: &[u8]| {
    let mut decoder = CapsuleDecoder::new();
    decoder.feed(data);

    // 全ての Capsule をデコードし尽くすまで繰り返す
    loop {
        match decoder.decode() {
            Ok(Some(_capsule)) => {
                // デコード成功、次の Capsule へ
            }
            Ok(None) => {
                // データ不足、終了
                break;
            }
            Err(_) => {
                // エラー、終了
                break;
            }
        }
    }
});
