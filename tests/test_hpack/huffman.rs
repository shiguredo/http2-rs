use shiguredo_http2::hpack::huffman::{decode, encode, encoded_len};

#[test]
fn test_encode_decode_simple() {
    let input = b"www.example.com";
    let encoded_length = encoded_len(input);
    let mut buf = vec![0u8; encoded_length];

    let len = encode(&mut buf, input).unwrap();
    assert_eq!(len, encoded_length);

    let decoded = decode(&buf).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn test_encode_decode_method() {
    let input = b"GET";
    let mut buf = vec![0u8; encoded_len(input)];

    encode(&mut buf, input).unwrap();
    let decoded = decode(&buf).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn test_encode_decode_path() {
    let input = b"/index.html";
    let mut buf = vec![0u8; encoded_len(input)];

    encode(&mut buf, input).unwrap();
    let decoded = decode(&buf).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn test_encoded_len() {
    // RFC 7541 Appendix C.4.1: "www.example.com" の Huffman 符号は 12 バイト
    assert_eq!(encoded_len(b"www.example.com"), 12);
}

#[test]
fn test_encode_buffer_too_short() {
    let input = b"test";
    let mut buf = [0u8; 1];
    assert!(encode(&mut buf, input).is_err());
}
