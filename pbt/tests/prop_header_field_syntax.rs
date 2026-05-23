//! const fn 検査 (`src/hpack/bytes.rs`) と runtime 検査 (`src/hpack/table.rs`) の
//! 同値性プロパティ
//!
//! 両者は実装が独立しているため、任意バイト列に対する accept/reject 判定が一致する
//! ことを proptest で検証する。乖離が出た場合は本 PBT が落ち、`validate_*` と
//! `check_*_const` のどちらか一方の修正漏れを検知できる。
//!
//! strategy は random バイト列、境界値 (NUL/HTAB/SP/CR/LF/`:`/大文字 ALPHA/数字 3 桁等)、
//! 既知 pseudo-header 名と典型値を `prop_oneof!` で混合し、shrinking 効率と境界値
//! カバレッジの両立を狙う。

use proptest::prelude::*;
use shiguredo_http2::__test_helpers as helpers;

/// 短いランダムバイト列
fn random_bytes(max_len: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..=max_len)
}

/// field-name の入力 strategy
///
/// ランダムバイト列に加えて、境界値 (空、`:` 単独、token / 非 token、大文字混在、
/// `:method` 等の既知 pseudo-header 名) を混在させる。
fn name_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        Just(Vec::<u8>::new()),
        Just(b":".to_vec()),
        Just(b":method".to_vec()),
        Just(b":scheme".to_vec()),
        Just(b":authority".to_vec()),
        Just(b":path".to_vec()),
        Just(b":status".to_vec()),
        Just(b":protocol".to_vec()),
        Just(b":unknown".to_vec()),
        Just(b"Host".to_vec()),
        Just(b"content-type".to_vec()),
        Just(b"X-Inject\r\nname".to_vec()),
        random_bytes(32),
    ]
}

/// field-value の入力 strategy
fn value_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        Just(Vec::<u8>::new()),
        Just(b" ".to_vec()),
        Just(b"\t".to_vec()),
        Just(b" GET".to_vec()),
        Just(b"GET ".to_vec()),
        Just(b"GET\r\n".to_vec()),
        Just(b"\0".to_vec()),
        Just(b"200".to_vec()),
        Just(b"199".to_vec()),
        Just(b"99".to_vec()),
        Just(b"https".to_vec()),
        Just(b"/foo".to_vec()),
        Just(b"*".to_vec()),
        Just(b"webtransport".to_vec()),
        random_bytes(48),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, ..ProptestConfig::default() })]

    /// field-name の検査結果が const と runtime で一致する
    #[test]
    fn prop_check_field_name_equivalence(name in name_strategy()) {
        let const_r = helpers::check_field_name_const_result(&name);
        let runtime_r = helpers::validate_field_name_result(&name);
        prop_assert_eq!(
            const_r.is_err(),
            runtime_r.is_err(),
            "field-name check mismatch for {:?}: const={:?} runtime={:?}",
            name,
            const_r,
            runtime_r
        );
    }

    /// field-value の検査結果が const と runtime で一致する
    #[test]
    fn prop_check_field_value_equivalence(value in value_strategy()) {
        let const_r = helpers::check_field_value_const_result(&value);
        // runtime 版は name 引数を取るがエラー文の組み立てにしか使わないため固定値
        let runtime_r = helpers::validate_field_value_result(b"x-test", &value);
        prop_assert_eq!(
            const_r.is_err(),
            runtime_r.is_err(),
            "field-value check mismatch for {:?}: const={:?} runtime={:?}",
            value,
            const_r,
            runtime_r
        );
    }

    /// pseudo-header の検査結果が const と runtime で一致する
    #[test]
    fn prop_check_pseudo_header_equivalence(
        name in name_strategy(),
        value in value_strategy(),
    ) {
        let const_r = helpers::check_pseudo_header_const_result(&name, &value);
        let runtime_r = helpers::validate_pseudo_header_result(&name, &value);
        prop_assert_eq!(
            const_r.is_err(),
            runtime_r.is_err(),
            "pseudo-header check mismatch for name={:?} value={:?}: const={:?} runtime={:?}",
            name,
            value,
            const_r,
            runtime_r
        );
    }
}
