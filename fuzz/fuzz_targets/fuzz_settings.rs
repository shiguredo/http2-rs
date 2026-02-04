#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{Setting, Settings};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    settings: Vec<FuzzSetting>,
}

#[derive(Debug, Arbitrary)]
struct FuzzSetting {
    id: u16,
    value: u32,
}

fuzz_target!(|input: FuzzInput| {
    let mut settings = Settings::new();

    // 任意の Setting を連続適用する
    // 境界値検証の漏れを検出する
    for s in &input.settings {
        let _ = settings.apply(Setting::new(s.id, s.value));
    }
});
