//! HPACK モジュール群の PBT
//!
//! `src/hpack/` ディレクトリモジュール配下のサブモジュールに対応する PBT を集約する。

mod decoder;
mod dynamic_table;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use shiguredo_http2::{HeaderField, HpackDecoder, HpackEncoder};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// 有効なヘッダー名を生成する (小文字 ASCII)
fn sample_valid_header_name(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-_";
    let len = noprop::sample_usize_in(ctx, 1..=32);
    (0..len)
        .map(|_| noprop::sample_choice(ctx, CHARSET))
        .collect()
}

/// 有効なヘッダー値を生成する (visible ASCII + 内部 SP/HTAB 許容、両端 SP/HTAB は除去)
///
/// RFC 9113 §8.2.1: field-value は内部 SP/HTAB を含んでよいが、両端は不可。
/// NUL/CR/LF は構築時検査で禁止される。
fn sample_valid_header_value(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    let len = noprop::sample_usize_in(ctx, 0..=64);
    let v: Vec<u8> = (0..len)
        .map(|_| noprop::sample_u64_in(ctx, 0x20..=0x7E) as u8)
        .collect();
    v.iter()
        .position(|&b| b != 0x20 && b != 0x09)
        .map(|start| {
            let end = v
                .iter()
                .rposition(|&b| b != 0x20 && b != 0x09)
                .expect("should succeed");
            v[start..=end].to_vec()
        })
        .unwrap_or_default()
}

/// 任意のバイト列 (0..=max_len) を生成する
fn sample_arbitrary_bytes(ctx: &mut noprop::TestCaseContext, max_len: usize) -> Vec<u8> {
    let len = noprop::sample_usize_in(ctx, 0..=max_len);
    noprop::sample_bytes_vec(ctx, len)
}

/// HPACK エンコード/デコードの往復テスト
#[test]
fn prop_hpack_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let count = noprop::sample_usize_in(ctx, 1..=8);
        let headers: Vec<HeaderField> = (0..count)
            .map(|_| {
                let name = sample_valid_header_name(ctx);
                let value = sample_valid_header_value(ctx);
                HeaderField::new(name, value).expect("valid header field")
            })
            .collect();

        let mut encoder = HpackEncoder::new(4096);
        let mut decoder = HpackDecoder::new(4096);

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        let decoded = decoder.decode(&encoded).expect("encode should succeed");

        assert_eq!(decoded.len(), headers.len());
        for (original, decoded) in headers.iter().zip(decoded.iter()) {
            assert_eq!(original.name(), decoded.name());
            assert_eq!(original.value(), decoded.value());
        }
        Ok(())
    })?;
    Ok(())
}

/// HPACK 整数エンコード/デコードの往復テスト
#[test]
fn prop_integer_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = noprop::sample_u32(ctx) as u64;
        let prefix_bits = 1 + noprop::sample_u64_in(ctx, 0..=7) as u8;
        let mut buf = [0u8; 16];
        let encoded_len = shiguredo_http2::hpack::integer::encode(&mut buf, value, prefix_bits, 0)
            .expect("should succeed");

        let (decoded, decoded_len) =
            shiguredo_http2::hpack::integer::decode(&buf, prefix_bits).expect("should succeed");

        assert_eq!(value, decoded);
        assert_eq!(encoded_len, decoded_len);
        Ok(())
    })?;
    Ok(())
}

/// Huffman エンコード/デコードの往復テスト
#[test]
fn prop_huffman_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_arbitrary_bytes(ctx, 128);
        let encoded = shiguredo_http2::hpack::huffman::encode_to_vec(&data);
        let decoded = shiguredo_http2::hpack::huffman::decode(&encoded).expect("should succeed");

        assert_eq!(data, decoded);
        Ok(())
    })?;
    Ok(())
}

/// Huffman エンコード長が元データ長を大幅に超えないことを確認
#[test]
fn prop_huffman_encoded_len_bounds() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_arbitrary_bytes(ctx, 128);
        let encoded_len = shiguredo_http2::hpack::huffman::encoded_len(&data);
        // 最悪ケースでも 30 ビット (RFC 7541 Appendix B の最大符号長) / 8 ビット = 3.75 倍程度
        assert!(encoded_len <= data.len() * 4 + 1);
        Ok(())
    })?;
    Ok(())
}

/// 動的テーブルのサイズ管理テスト
#[test]
fn prop_dynamic_table_size_management() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let count = noprop::sample_usize_in(ctx, 1..=16);
        let headers: Vec<HeaderField> = (0..count)
            .map(|_| {
                let name = sample_valid_header_name(ctx);
                let value = sample_valid_header_value(ctx);
                HeaderField::new(name, value).expect("valid header field")
            })
            .collect();
        let max_size = noprop::sample_usize_in(ctx, 64..=4096);

        let mut encoder = HpackEncoder::new(max_size);

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        // 動的テーブルのサイズが最大サイズを超えていないことを確認
        assert!(encoder.dynamic_table().size() <= max_size);
        Ok(())
    })?;
    Ok(())
}

/// 機密ヘッダー (Never Indexed) のエンコード/デコードテスト
#[test]
fn prop_sensitive_header_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let count = noprop::sample_usize_in(ctx, 1..=8);
        let headers: Vec<HeaderField> = (0..count)
            .map(|_| {
                let name = sample_valid_header_name(ctx);
                let value = sample_valid_header_value(ctx);
                let sensitive = noprop::sample_bool(ctx);
                HeaderField::new_with_sensitive(name, value, sensitive).expect("valid header field")
            })
            .collect();

        let mut encoder = HpackEncoder::new(4096);
        let mut decoder = HpackDecoder::new(4096);

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        let decoded = decoder.decode(&encoded).expect("encode should succeed");

        assert_eq!(decoded.len(), headers.len());
        for (original, decoded) in headers.iter().zip(decoded.iter()) {
            assert_eq!(original.name(), decoded.name());
            assert_eq!(original.value(), decoded.value());
            assert_eq!(original.sensitive(), decoded.sensitive());
        }
        Ok(())
    })?;
    Ok(())
}

/// HPACK エンコード/デコード往復後の HeaderField 等価性テスト
///
/// `new` で構築した HeaderField と HPACK encode/decode で再構築した HeaderField の
/// PartialEq / Hash / size() が一致することを検証する。
/// Cow::Borrowed vs Cow::Owned の cross-variant 等価性は
/// `src/hpack/table.rs` の `header_field_cross_variant_*` 単体テストで検証する。
#[test]
fn prop_header_field_hpack_roundtrip_equivalence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let name = sample_valid_header_name(ctx);
        let value = sample_valid_header_value(ctx);
        let runtime = HeaderField::new(&name, &value).expect("valid header field");

        // HPACK Literal Header Field without Indexing として符号化し、
        // decoder 経由で Cow::Owned な HeaderField を構築する
        let mut encoder = HpackEncoder::new(0);
        let mut decoder = HpackDecoder::new(0);
        let headers = vec![runtime.clone()];
        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);
        let decoded = decoder.decode(&encoded).expect("valid HPACK");
        let decoded_field = &decoded[0];

        // PartialEq
        assert_eq!(&runtime, decoded_field);

        // Hash
        let mut h1 = DefaultHasher::new();
        runtime.hash(&mut h1);
        let mut h2 = DefaultHasher::new();
        decoded_field.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());

        // size()
        assert_eq!(runtime.size(), decoded_field.size());
        Ok(())
    })?;
    Ok(())
}

/// 機密ヘッダーは動的テーブルに追加されないことを確認
#[test]
fn prop_sensitive_headers_not_indexed() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let name = sample_valid_header_name(ctx);
        let value = sample_valid_header_value(ctx);
        let mut encoder = HpackEncoder::new(4096);
        let mut decoder = HpackDecoder::new(4096);

        // 機密ヘッダーのみをエンコード
        let headers = vec![
            HeaderField::new_with_sensitive(name.clone(), value.clone(), true)
                .expect("should succeed"),
        ];

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        // デコード後、動的テーブルは空のままであること
        let _ = decoder.decode(&encoded).expect("decode should succeed");
        assert_eq!(decoder.dynamic_table().len(), 0);

        // エンコーダーの動的テーブルも空のままであること
        assert_eq!(encoder.dynamic_table().len(), 0);
        Ok(())
    })?;
    Ok(())
}
