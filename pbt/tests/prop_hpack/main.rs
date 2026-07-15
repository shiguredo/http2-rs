//! HPACK モジュール群の PBT
//!
//! `src/hpack/` ディレクトリモジュール配下のサブモジュールに対応する PBT を集約する。

mod decoder;
mod dynamic_table;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use proptest::prelude::*;
use shiguredo_http2::{HeaderField, HpackDecoder, HpackEncoder};

/// 有効なヘッダー名を生成する（小文字 ASCII）
fn valid_header_name() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        prop::sample::select(
            (b'a'..=b'z')
                .chain(b'0'..=b'9')
                .chain(*b"-_")
                .collect::<Vec<_>>(),
        ),
        1..=32,
    )
}

/// 有効なヘッダー値を生成する (visible ASCII + 内部 SP/HTAB 許容、両端 SP/HTAB は除去)
///
/// RFC 9113 §8.2.1: field-value は内部 SP/HTAB を含んでよいが、両端は不可。
/// NUL/CR/LF は構築時検査で禁止される。
fn valid_header_value() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0x20u8..=0x7Eu8, 0..=64).prop_map(|v| {
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
    })
}

/// 任意のバイト列を生成する
fn arbitrary_bytes(max_len: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..=max_len)
}

proptest! {
    /// HPACK エンコード/デコードの往復テスト
    #[test]
    fn prop_hpack_roundtrip(
        headers in prop::collection::vec(
            (valid_header_name(), valid_header_value()),
            1..=8
        )
    ) {
        let headers: Vec<HeaderField> = headers
            .into_iter()
            .map(|(name, value)| HeaderField::new(name, value).expect("valid header field"))
            .collect();

        let mut encoder = HpackEncoder::new(4096);
        let mut decoder = HpackDecoder::new(4096);

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        let decoded = decoder.decode(&encoded).expect("encode should succeed");

        prop_assert_eq!(decoded.len(), headers.len());
        for (original, decoded) in headers.iter().zip(decoded.iter()) {
            prop_assert_eq!(original.name(), decoded.name());
            prop_assert_eq!(original.value(), decoded.value());
        }
    }

    /// HPACK 整数エンコード/デコードの往復テスト
    #[test]
    fn prop_integer_roundtrip(
        value in 0u64..=0xFFFF_FFFFu64,
        prefix_bits in 1u8..=8u8,
    ) {
        let mut buf = [0u8; 16];
        let encoded_len = shiguredo_http2::hpack::integer::encode(
            &mut buf, value, prefix_bits, 0
        ).expect("should succeed");

        let (decoded, decoded_len) = shiguredo_http2::hpack::integer::decode(
            &buf, prefix_bits
        ).expect("should succeed");

        prop_assert_eq!(value, decoded);
        prop_assert_eq!(encoded_len, decoded_len);
    }

    /// Huffman エンコード/デコードの往復テスト
    #[test]
    fn prop_huffman_roundtrip(data in arbitrary_bytes(128)) {
        let encoded = shiguredo_http2::hpack::huffman::encode_to_vec(&data);
        let decoded = shiguredo_http2::hpack::huffman::decode(&encoded).expect("should succeed");

        prop_assert_eq!(data, decoded);
    }

    /// Huffman エンコード長が元データ長を大幅に超えないことを確認
    #[test]
    fn prop_huffman_encoded_len_bounds(data in arbitrary_bytes(128)) {
        let encoded_len = shiguredo_http2::hpack::huffman::encoded_len(&data);
        // 最悪ケースでも 30 ビット (RFC 7541 Appendix B の最大符号長) / 8 ビット = 3.75 倍程度
        prop_assert!(encoded_len <= data.len() * 4 + 1);
    }

    /// 動的テーブルのサイズ管理テスト
    #[test]
    fn prop_dynamic_table_size_management(
        headers in prop::collection::vec(
            (valid_header_name(), valid_header_value()),
            1..=16
        ),
        max_size in 64usize..=4096usize,
    ) {
        let headers: Vec<HeaderField> = headers
            .into_iter()
            .map(|(name, value)| HeaderField::new(name, value).expect("valid header field"))
            .collect();

        let mut encoder = HpackEncoder::new(max_size);

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        // 動的テーブルのサイズが最大サイズを超えていないことを確認
        prop_assert!(encoder.dynamic_table().size() <= max_size);
    }

    /// 機密ヘッダー (Never Indexed) のエンコード/デコードテスト
    #[test]
    fn prop_sensitive_header_roundtrip(
        headers in prop::collection::vec(
            (valid_header_name(), valid_header_value(), any::<bool>()),
            1..=8
        )
    ) {
        let headers: Vec<HeaderField> = headers
            .into_iter()
            .map(|(name, value, sensitive)| {
                HeaderField::new_with_sensitive(name, value, sensitive).expect("valid header field")
            })
            .collect();

        let mut encoder = HpackEncoder::new(4096);
        let mut decoder = HpackDecoder::new(4096);

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        let decoded = decoder.decode(&encoded).expect("encode should succeed");

        prop_assert_eq!(decoded.len(), headers.len());
        for (original, decoded) in headers.iter().zip(decoded.iter()) {
            prop_assert_eq!(original.name(), decoded.name());
            prop_assert_eq!(original.value(), decoded.value());
            prop_assert_eq!(original.sensitive(), decoded.sensitive());
        }
    }

    /// HPACK エンコード/デコード往復後の HeaderField 等価性テスト
    ///
    /// `new` で構築した HeaderField と HPACK encode/decode で再構築した HeaderField の
    /// PartialEq / Hash / size() が一致することを検証する。
    /// Cow::Borrowed vs Cow::Owned の cross-variant 等価性は
    /// `src/hpack/table.rs` の `header_field_cross_variant_*` 単体テストで検証する。
    #[test]
    fn prop_header_field_hpack_roundtrip_equivalence(
        name in valid_header_name(),
        value in valid_header_value(),
    ) {
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
        prop_assert_eq!(&runtime, decoded_field);

        // Hash
        let mut h1 = DefaultHasher::new();
        runtime.hash(&mut h1);
        let mut h2 = DefaultHasher::new();
        decoded_field.hash(&mut h2);
        prop_assert_eq!(h1.finish(), h2.finish());

        // size()
        prop_assert_eq!(runtime.size(), decoded_field.size());
    }

    /// 機密ヘッダーは動的テーブルに追加されないことを確認
    #[test]
    fn prop_sensitive_headers_not_indexed(
        name in valid_header_name(),
        value in valid_header_value(),
    ) {
        let mut encoder = HpackEncoder::new(4096);
        let mut decoder = HpackDecoder::new(4096);

        // 機密ヘッダーのみをエンコード
        let headers = vec![HeaderField::new_with_sensitive(
            name.clone(),
            value.clone(),
            true,
        )
        .expect("should succeed")];

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        // デコード後、動的テーブルは空のままであること
        let _ = decoder.decode(&encoded).expect("decode should succeed");
        prop_assert_eq!(decoder.dynamic_table().len(), 0);

        // エンコーダーの動的テーブルも空のままであること
        prop_assert_eq!(encoder.dynamic_table().len(), 0);
    }
}
