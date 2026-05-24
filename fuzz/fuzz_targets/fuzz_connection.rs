#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{Connection, Limits};

fuzz_target!(|data: &[u8]| {
    // サーバーとして任意のバイト列を処理する
    // クライアントから送られる任意のフレーム列による状態遷移のパニックを検出する
    let limits = Limits::default();
    let mut conn = Connection::server(limits);

    // プリフェイスは外部で処理済みとしてマークする
    conn.mark_preface_received();

    // SETTINGS を送信済みとする
    let _ = conn.initiate();

    // 出力バッファを消費する
    while conn.poll_output().is_some() {}

    // 任意のバイト列を feed して process する
    let _ = conn.feed(data);
    let _ = conn.process();

    // イベントを全て消費する
    while conn.poll_event().is_some() {}

    // 出力バッファを全て消費する
    while conn.poll_output().is_some() {}
});
