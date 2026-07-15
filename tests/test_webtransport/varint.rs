use shiguredo_http2::webtransport::varint::{
    MAX_VALUE, decode, encode, encode_to_vec, encoded_len,
};

#[test]
fn test_encoded_len() {
    // RFC 9000 Section 16 Table 4: 1/2/4/8 バイトでそれぞれ 0-63 / 0-16383 / 0-1073741823 / 0-2^62-1 を表現する
    assert_eq!(encoded_len(0), 1);
    assert_eq!(encoded_len(63), 1);
    assert_eq!(encoded_len(64), 2);
    assert_eq!(encoded_len(16383), 2);
    assert_eq!(encoded_len(16384), 4);
    assert_eq!(encoded_len(1073741823), 4);
    assert_eq!(encoded_len(1073741824), 8);
    assert_eq!(encoded_len(MAX_VALUE), 8);
}

#[test]
fn test_encode_decode_1_byte() {
    let mut buf = [0u8; 8];

    for value in [0, 1, 37, 63] {
        let len = encode(value, &mut buf).expect("should succeed");
        assert_eq!(len, 1);

        let (decoded, consumed) = decode(&buf[..len]).expect("should succeed");
        assert_eq!(decoded, value);
        assert_eq!(consumed, 1);
    }
}

#[test]
fn test_encode_decode_2_bytes() {
    let mut buf = [0u8; 8];

    for value in [64, 100, 494, 16383] {
        let len = encode(value, &mut buf).expect("should succeed");
        assert_eq!(len, 2);

        let (decoded, consumed) = decode(&buf[..len]).expect("should succeed");
        assert_eq!(decoded, value);
        assert_eq!(consumed, 2);
    }
}

#[test]
fn test_encode_decode_4_bytes() {
    let mut buf = [0u8; 8];

    for value in [16384, 65535, 494878333, 1073741823] {
        let len = encode(value, &mut buf).expect("should succeed");
        assert_eq!(len, 4);

        let (decoded, consumed) = decode(&buf[..len]).expect("should succeed");
        assert_eq!(decoded, value);
        assert_eq!(consumed, 4);
    }
}

#[test]
fn test_encode_decode_8_bytes() {
    let mut buf = [0u8; 8];

    for value in [1073741824, 151288809941952652, MAX_VALUE] {
        let len = encode(value, &mut buf).expect("should succeed");
        assert_eq!(len, 8);

        let (decoded, consumed) = decode(&buf[..len]).expect("should succeed");
        assert_eq!(decoded, value);
        assert_eq!(consumed, 8);
    }
}

#[test]
fn test_encode_overflow() {
    let mut buf = [0u8; 8];
    let result = encode(MAX_VALUE + 1, &mut buf);
    assert!(result.is_err());
}

#[test]
fn test_encode_buffer_too_short() {
    let mut buf = [0u8; 1];
    let result = encode(16384, &mut buf);
    assert!(result.is_err());
}

#[test]
fn test_decode_incomplete() {
    // 空バッファ
    assert!(decode(&[]).is_err());

    // 2 バイトが必要だが 1 バイトしかない
    assert!(decode(&[0x40]).is_err());

    // 4 バイトが必要だが 3 バイトしかない
    assert!(decode(&[0x80, 0x00, 0x00]).is_err());

    // 8 バイトが必要だが 7 バイトしかない
    assert!(decode(&[0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).is_err());
}

#[test]
fn test_decode_non_minimal_encoding() {
    // RFC 9000 Section 16 / RFC 9297 は非最小エンコーディングを許容するが、本実装は方針として拒否する

    // 値 10 を 2 バイトでエンコード (最小は 1 バイト)
    // 0x40 | (10 >> 8) = 0x40, buf[1] = 10
    assert!(decode(&[0x40, 0x0a]).is_err());

    // 値 0 を 2 バイトでエンコード (最小は 1 バイト)
    assert!(decode(&[0x40, 0x00]).is_err());

    // 値 63 を 2 バイトでエンコード (最小は 1 バイト)
    assert!(decode(&[0x40, 0x3f]).is_err());

    // 値 64 を 4 バイトでエンコード (最小は 2 バイト)
    assert!(decode(&[0x80, 0x00, 0x00, 0x40]).is_err());

    // 値 16383 を 4 バイトでエンコード (最小は 2 バイト)
    assert!(decode(&[0x80, 0x00, 0x3f, 0xff]).is_err());

    // 値 16384 を 8 バイトでエンコード (最小は 4 バイト)
    assert!(decode(&[0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00]).is_err());
}

#[test]
fn test_rfc_examples() {
    // RFC 9000 Appendix A.1 のサンプル値
    let mut buf = [0u8; 8];

    // 37 -> 0x25 (1 byte)
    let len = encode(37, &mut buf).expect("should succeed");
    assert_eq!(&buf[..len], &[0x25]);

    // 15293 -> 0x7bbd (2 bytes)
    let len = encode(15293, &mut buf).expect("should succeed");
    assert_eq!(&buf[..len], &[0x7b, 0xbd]);

    // 494878333 -> 0x9d7f3e7d (4 bytes)
    let len = encode(494878333, &mut buf).expect("should succeed");
    assert_eq!(&buf[..len], &[0x9d, 0x7f, 0x3e, 0x7d]);

    // 151288809941952652 -> 0xc2197c5eff14e88c (8 bytes)
    let len = encode(151288809941952652, &mut buf).expect("should succeed");
    assert_eq!(
        &buf[..len],
        &[0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c]
    );
}

#[test]
fn test_encode_to_vec() {
    let buf = encode_to_vec(37).expect("should succeed");
    assert_eq!(buf, vec![0x25]);

    let buf = encode_to_vec(15293).expect("should succeed");
    assert_eq!(buf, vec![0x7b, 0xbd]);
}
