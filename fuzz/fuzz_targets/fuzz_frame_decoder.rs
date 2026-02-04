#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::FrameDecoder;

fuzz_target!(|data: &[u8]| {
    let mut decoder = FrameDecoder::new(16384);
    decoder.feed(data);

    // 全てのフレームをデコードし尽くすまで繰り返す
    loop {
        match decoder.decode() {
            Ok(Some(_frame)) => {
                // デコード成功、次のフレームへ
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
