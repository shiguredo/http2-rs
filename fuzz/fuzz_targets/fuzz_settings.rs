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

    // 任意の wire 値を Setting::from_wire で検証し、有効なものだけ apply する
    for s in &input.settings {
        if let Ok(setting) = Setting::from_wire(s.id, s.value) {
            settings.apply(setting);
        }
    }
});
