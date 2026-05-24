#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{Connection, Limits};

fuzz_target!(|data: &[u8]| {
    // クライアントとして任意のバイト列を処理する
    // サーバーから送られる任意のフレーム列による状態遷移のパニックを検出する
    let limits = Limits::default();
    let mut conn = Connection::client(limits);

    // クライアント接続プリフェイスと SETTINGS を送信する
    let _ = conn.initiate();

    // 出力バッファを消費する
    while conn.poll_output().is_some() {}

    // サーバーからの任意バイト列を feed して process する
    let _ = conn.feed(data);
    let _ = conn.process();

    // イベントを全て消費する
    while conn.poll_event().is_some() {}

    // 出力バッファを全て消費する
    while conn.poll_output().is_some() {}
});
