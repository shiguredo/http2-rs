//! PBT / fuzz 専用の crate 内部 API 公開モジュール
//!
//! `__test_helpers` cargo feature 有効時のみコンパイルされる。
//! 構築時検査関数 (const fn 版 / runtime 版) の同値性を PBT で検証するための薄いラッパと、
//! HPACK decoder 経路を再現するための `HeaderField::from_validated_parts` への
//! 再エクスポート系ラッパを提供する。
//! 本番利用者は本 feature を有効化してはならない (型不変条件を破壊する)。
//!
//! panic catch ラッパは戻り値を `Result<(), String>` で返し、`Err` に panic メッセージを
//! 保持して proptest 失敗時に乖離の理由を直接表示できるようにする。catch_unwind 中の
//! stderr 汚染を抑止するため、モジュール初期化時に panic hook を 1 度だけ無音化する
//! (proptest 並列実行下でもグローバルに 1 度のみ設置、`Once` で他テストへの副作用を制限)。

use std::panic;
use std::sync::Once;

/// panic hook を 1 度だけ無音化する
fn install_silent_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        panic::set_hook(Box::new(|_| {}));
    });
}

fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// `check_field_name_const` を runtime 評価し、panic 時はメッセージを返す
pub fn check_field_name_const_result(name: &[u8]) -> Result<(), String> {
    install_silent_panic_hook();
    panic::catch_unwind(panic::AssertUnwindSafe(|| {
        crate::hpack::bytes::check_field_name_const(name);
    }))
    .map_err(panic_payload_message)
}

/// `validate_field_name` の結果を文字列化された Err として返す
pub fn validate_field_name_result(name: &[u8]) -> Result<(), String> {
    crate::hpack::table::validate_field_name(name).map_err(|e| format!("{e}"))
}

/// `check_field_value_const` を runtime 評価し、panic 時はメッセージを返す
pub fn check_field_value_const_result(value: &[u8]) -> Result<(), String> {
    install_silent_panic_hook();
    panic::catch_unwind(panic::AssertUnwindSafe(|| {
        crate::hpack::bytes::check_field_value_const(value);
    }))
    .map_err(panic_payload_message)
}

/// `validate_field_value` の結果を文字列化された Err として返す
pub fn validate_field_value_result(name: &[u8], value: &[u8]) -> Result<(), String> {
    crate::hpack::table::validate_field_value(name, value).map_err(|e| format!("{e}"))
}

/// `check_pseudo_header_const` を runtime 評価し、panic 時はメッセージを返す
pub fn check_pseudo_header_const_result(name: &[u8], value: &[u8]) -> Result<(), String> {
    install_silent_panic_hook();
    panic::catch_unwind(panic::AssertUnwindSafe(|| {
        crate::hpack::bytes::check_pseudo_header_const(name, value);
    }))
    .map_err(panic_payload_message)
}

/// `validate_pseudo_header` の結果を文字列化された Err として返す
pub fn validate_pseudo_header_result(name: &[u8], value: &[u8]) -> Result<(), String> {
    crate::hpack::table::validate_pseudo_header(name, value).map_err(|e| format!("{e}"))
}

/// HPACK decoder 経路の `HeaderField::from_validated_parts` を PBT / fuzz から呼べるよう公開する
///
/// 検査をバイパスして `HeaderField` を構築するため、型不変条件を破壊しうる。
/// 本番利用禁止。
pub fn header_field_from_validated_parts(
    name: Vec<u8>,
    value: Vec<u8>,
    sensitive: bool,
) -> crate::hpack::HeaderField {
    crate::hpack::HeaderField::from_validated_parts(name, value, sensitive)
}
