use shiguredo_http2::hpack::integer::{decode, encode};

#[test]
fn test_encode_small_value() {
    let mut buf = [0u8; 8];
    let len = encode(&mut buf, 10, 5, 0).unwrap();
    assert_eq!(len, 1);
    assert_eq!(buf[0], 10);
}

#[test]
fn test_encode_max_prefix() {
    let mut buf = [0u8; 8];
    let len = encode(&mut buf, 31, 5, 0).unwrap();
    assert_eq!(len, 2);
    assert_eq!(buf[0], 31);
    assert_eq!(buf[1], 0);
}

#[test]
fn test_encode_large_value() {
    // RFC 7541 Appendix C.1.2 の例: 1337 を 5 ビットプレフィックスでエンコード
    let mut buf = [0u8; 8];
    let len = encode(&mut buf, 1337, 5, 0).unwrap();
    assert_eq!(len, 3);
    assert_eq!(buf[0], 31);
    assert_eq!(buf[1], 154);
    assert_eq!(buf[2], 10);
}

#[test]
fn test_decode_small_value() {
    let buf = [10u8];
    let (value, len) = decode(&buf, 5).unwrap();
    assert_eq!(value, 10);
    assert_eq!(len, 1);
}

#[test]
fn test_decode_large_value() {
    // RFC 7541 Appendix C.1.2 の例: 1337
    let buf = [31u8, 154, 10];
    let (value, len) = decode(&buf, 5).unwrap();
    assert_eq!(value, 1337);
    assert_eq!(len, 3);
}

#[test]
fn test_roundtrip() {
    for value in [0, 1, 30, 31, 127, 128, 1337, 65535, 1_000_000] {
        for prefix_bits in 1..=8 {
            let mut buf = [0u8; 16];
            let encoded_len = encode(&mut buf, value, prefix_bits, 0).unwrap();
            let (decoded, decoded_len) = decode(&buf, prefix_bits).unwrap();
            assert_eq!(value, decoded);
            assert_eq!(encoded_len, decoded_len);
        }
    }
}
