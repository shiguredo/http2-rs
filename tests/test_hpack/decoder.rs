use shiguredo_http2::hpack::{Decoder, Encoder, HeaderField};

#[test]
fn test_decode_indexed() {
    let mut decoder = Decoder::new(4096);

    // :method: GET (index 2) = 0x82
    let data = [0x82];
    let headers = decoder.decode(&data).unwrap();

    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name(), b":method");
    assert_eq!(headers[0].value(), b"GET");
}

#[test]
fn test_decode_literal_indexed() {
    let mut decoder = Decoder::new(4096);

    // :authority (index 1) with value "example.com" (not Huffman encoded)
    // 0x41 = 01000001 (incremental indexing, index 1)
    // 0x0b = length 11
    // "example.com"
    let mut data = vec![0x41, 0x0b];
    data.extend_from_slice(b"example.com");

    let headers = decoder.decode(&data).unwrap();

    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name(), b":authority");
    assert_eq!(headers[0].value(), b"example.com");

    // 動的テーブルに追加されていることを確認
    assert_eq!(decoder.dynamic_table().len(), 1);
}

#[test]
fn test_roundtrip() {
    let mut encoder = Encoder::new(4096);
    let mut decoder = Decoder::new(4096);

    let headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new(":path", "/index.html").unwrap(),
        HeaderField::new(":scheme", "https").unwrap(),
        HeaderField::new(":authority", "www.example.com").unwrap(),
        HeaderField::new("custom-header", "custom-value").unwrap(),
    ];

    let mut encoded = Vec::new();
    encoder.encode(&mut encoded, &headers);

    let decoded = decoder.decode(&encoded).unwrap();

    assert_eq!(decoded.len(), headers.len());
    for (original, decoded) in headers.iter().zip(decoded.iter()) {
        assert_eq!(original.name(), decoded.name());
        assert_eq!(original.value(), decoded.value());
    }
}

#[test]
fn test_decode_size_update() {
    let mut decoder = Decoder::new(4096);

    // Size update to 1024 = 0x3f (5-bit prefix) + continuation
    // 0x20 | (31 & 0x1f) = 0x3f, then 1024 - 31 = 993 = 0xe1 0x07
    let data = [0x3f, 0xe1, 0x07];
    let headers = decoder.decode(&data).unwrap();

    assert!(headers.is_empty());
    assert_eq!(decoder.dynamic_table().max_size(), 1024);
}

#[test]
fn test_decode_never_indexed() {
    let mut decoder = Decoder::new(4096);

    // Never Indexed with new name "x-token" and value "secret"
    // 0x10 = pattern 00010000 (never indexed, new name)
    // 0x07 = length 7 (not Huffman)
    // "x-token"
    // 0x06 = length 6 (not Huffman)
    // "secret"
    let mut data = vec![0x10, 0x07];
    data.extend_from_slice(b"x-token");
    data.push(0x06);
    data.extend_from_slice(b"secret");

    let headers = decoder.decode(&data).unwrap();

    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name(), b"x-token");
    assert_eq!(headers[0].value(), b"secret");
    assert!(headers[0].sensitive());

    // Never Indexed should not be added to dynamic table
    assert_eq!(decoder.dynamic_table().len(), 0);
}

#[test]
fn test_decode_never_indexed_with_name_index() {
    let mut decoder = Decoder::new(4096);

    // Never Indexed with name index 7 (:scheme) and value "https"
    // 0x17 = pattern 0001 + 0111 (never indexed, index 7)
    // 0x05 = length 5 (not Huffman)
    // "https"
    let mut data = vec![0x17, 0x05];
    data.extend_from_slice(b"https");

    let headers = decoder.decode(&data).unwrap();

    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name(), b":scheme");
    assert_eq!(headers[0].value(), b"https");
    assert!(headers[0].sensitive());
}

#[test]
fn test_roundtrip_with_sensitive() {
    let mut encoder = Encoder::new(4096);
    let mut decoder = Decoder::new(4096);

    let headers = vec![
        HeaderField::new(":method", "GET").unwrap(),
        HeaderField::new_with_sensitive("authorization", "Bearer token", true).unwrap(),
        HeaderField::new(":path", "/").unwrap(),
    ];

    let mut encoded = Vec::new();
    encoder.encode(&mut encoded, &headers);

    let decoded = decoder.decode(&encoded).unwrap();

    assert_eq!(decoded.len(), 3);
    assert_eq!(decoded[0].name(), b":method");
    assert!(!decoded[0].sensitive());
    assert_eq!(decoded[1].name(), b"authorization");
    assert!(decoded[1].sensitive());
    assert_eq!(decoded[2].name(), b":path");
    assert!(!decoded[2].sensitive());
}
