#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{Connection, Limits};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    chunks: Vec<Vec<u8>>,
}

fuzz_target!(|input: FuzzInput| {
    // サーバーロールで mark_preface_received() を呼ばずに、
    // 任意バイト列を断片的に feed する。
    // HTTP/2 接続プリフェイス (RFC 9113 §3.4) のバッファリングと
    // 検証の境界条件のパニック安全性を検証する。
    let limits = Limits::default();
    let mut conn = Connection::server(limits);

    // SETTINGS を送信済みとする
    let _ = conn.initiate();

    // 出力バッファを消費する
    while conn.poll_output().is_some() {}

    // chunks を先頭 64 件に切り詰める (fuzzer のスループット確保のため)
    for chunk in input.chunks.iter().take(64) {
        let _ = conn.feed(chunk);
        let _ = conn.process();
        while conn.poll_event().is_some() {}
        while conn.poll_output().is_some() {}
    }
});
