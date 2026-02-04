//! RFC 7541 Appendix C テストベクター
//!
//! HPACK の参照テストケースを使用して実装を検証する。

use shiguredo_http2::{HeaderField, HpackDecoder, HpackEncoder};

/// C.1.1 - Encoding 10 with 5-bit prefix
#[test]
fn test_c1_1_integer_encoding_10() {
    let mut buf = [0u8; 16];
    let len = shiguredo_http2::hpack::integer::encode(&mut buf, 10, 5, 0).unwrap();
    assert_eq!(len, 1);
    assert_eq!(buf[0], 0x0a); // 10
}

/// C.1.2 - Encoding 1337 with 5-bit prefix
#[test]
fn test_c1_2_integer_encoding_1337() {
    let mut buf = [0u8; 16];
    let len = shiguredo_http2::hpack::integer::encode(&mut buf, 1337, 5, 0).unwrap();
    assert_eq!(len, 3);
    assert_eq!(buf[0], 0x1f); // 31
    assert_eq!(buf[1], 0x9a); // 154
    assert_eq!(buf[2], 0x0a); // 10
}

/// C.1.3 - Encoding 42 starting at an octet boundary
#[test]
fn test_c1_3_integer_encoding_42() {
    let mut buf = [0u8; 16];
    let len = shiguredo_http2::hpack::integer::encode(&mut buf, 42, 8, 0).unwrap();
    assert_eq!(len, 1);
    assert_eq!(buf[0], 0x2a); // 42
}

/// C.2.1 - Literal Header Field with Indexing
#[test]
fn test_c2_1_literal_header_with_indexing() {
    let mut decoder = HpackDecoder::new(4096);

    // 400a 6375 7374 6f6d 2d6b 6579 0d63 7573
    // 746f 6d2d 6865 6164 6572
    let encoded = [
        0x40, 0x0a, 0x63, 0x75, 0x73, 0x74, 0x6f, 0x6d, 0x2d, 0x6b, 0x65, 0x79, 0x0d, 0x63, 0x75,
        0x73, 0x74, 0x6f, 0x6d, 0x2d, 0x68, 0x65, 0x61, 0x64, 0x65, 0x72,
    ];

    let headers = decoder.decode(&encoded).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name, b"custom-key");
    assert_eq!(headers[0].value, b"custom-header");

    // 動的テーブルにエントリが追加されていることを確認
    assert_eq!(decoder.dynamic_table().len(), 1);
}

/// C.2.2 - Literal Header Field without Indexing
#[test]
fn test_c2_2_literal_header_without_indexing() {
    let mut decoder = HpackDecoder::new(4096);

    // 040c 2f73 616d 706c 652f 7061 7468
    let encoded = [
        0x04, 0x0c, 0x2f, 0x73, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x2f, 0x70, 0x61, 0x74, 0x68,
    ];

    let headers = decoder.decode(&encoded).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name, b":path");
    assert_eq!(headers[0].value, b"/sample/path");

    // 動的テーブルにエントリが追加されていないことを確認
    assert_eq!(decoder.dynamic_table().len(), 0);
}

/// C.2.3 - Literal Header Field Never Indexed
#[test]
fn test_c2_3_literal_header_never_indexed() {
    let mut decoder = HpackDecoder::new(4096);

    // 1008 7061 7373 776f 7264 0673 6563 7265 74
    let encoded = [
        0x10, 0x08, 0x70, 0x61, 0x73, 0x73, 0x77, 0x6f, 0x72, 0x64, 0x06, 0x73, 0x65, 0x63, 0x72,
        0x65, 0x74,
    ];

    let headers = decoder.decode(&encoded).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name, b"password");
    assert_eq!(headers[0].value, b"secret");

    // 動的テーブルにエントリが追加されていないことを確認
    assert_eq!(decoder.dynamic_table().len(), 0);
}

/// C.2.4 - Indexed Header Field
#[test]
fn test_c2_4_indexed_header_field() {
    let mut decoder = HpackDecoder::new(4096);

    // 82
    let encoded = [0x82];

    let headers = decoder.decode(&encoded).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name, b":method");
    assert_eq!(headers[0].value, b"GET");
}

/// C.3.1 - First Request (without Huffman)
#[test]
fn test_c3_1_first_request() {
    let mut decoder = HpackDecoder::new(4096);

    let encoded = [
        0x82, // :method: GET
        0x86, // :scheme: http
        0x84, // :path: /
        0x41, // :authority (indexed name)
        0x0f, 0x77, 0x77, 0x77, 0x2e, 0x65, 0x78, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x2e, 0x63, 0x6f,
        0x6d, // www.example.com
    ];

    let headers = decoder.decode(&encoded).unwrap();
    assert_eq!(headers.len(), 4);

    assert_eq!(headers[0].name, b":method");
    assert_eq!(headers[0].value, b"GET");

    assert_eq!(headers[1].name, b":scheme");
    assert_eq!(headers[1].value, b"http");

    assert_eq!(headers[2].name, b":path");
    assert_eq!(headers[2].value, b"/");

    assert_eq!(headers[3].name, b":authority");
    assert_eq!(headers[3].value, b"www.example.com");
}

/// C.4.1 - First Request (with Huffman)
#[test]
fn test_c4_1_first_request_huffman() {
    let mut decoder = HpackDecoder::new(4096);

    let encoded = [
        0x82, // :method: GET
        0x86, // :scheme: http
        0x84, // :path: /
        0x41, // :authority (indexed name)
        0x8c, // Huffman encoded, length 12
        0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab, 0x90, 0xf4,
        0xff, // www.example.com (Huffman)
    ];

    let headers = decoder.decode(&encoded).unwrap();
    assert_eq!(headers.len(), 4);

    assert_eq!(headers[0].name, b":method");
    assert_eq!(headers[0].value, b"GET");

    assert_eq!(headers[1].name, b":scheme");
    assert_eq!(headers[1].value, b"http");

    assert_eq!(headers[2].name, b":path");
    assert_eq!(headers[2].value, b"/");

    assert_eq!(headers[3].name, b":authority");
    assert_eq!(headers[3].value, b"www.example.com");
}

/// C.5 - Response Examples without Huffman
#[test]
fn test_c5_1_first_response() {
    let mut decoder = HpackDecoder::new(256);

    // First Response
    let encoded1 = [
        0x48, // :status (indexed name, literal value)
        0x03, 0x33, 0x30, 0x32, // 302
        0x58, // cache-control (indexed name)
        0x07, 0x70, 0x72, 0x69, 0x76, 0x61, 0x74, 0x65, // private
        0x61, // date (indexed name)
        0x1d, 0x4d, 0x6f, 0x6e, 0x2c, 0x20, 0x32, 0x31, 0x20, 0x4f, 0x63, 0x74, 0x20, 0x32, 0x30,
        0x31, 0x33, 0x20, 0x32, 0x30, 0x3a, 0x31, 0x33, 0x3a, 0x32, 0x31, 0x20, 0x47, 0x4d,
        0x54, // Mon, 21 Oct 2013 20:13:21 GMT
        0x6e, // location (indexed name)
        0x17, 0x68, 0x74, 0x74, 0x70, 0x73, 0x3a, 0x2f, 0x2f, 0x77, 0x77, 0x77, 0x2e, 0x65, 0x78,
        0x61, 0x6d, 0x70, 0x6c, 0x65, 0x2e, 0x63, 0x6f, 0x6d, // https://www.example.com
    ];

    let headers1 = decoder.decode(&encoded1).unwrap();
    assert_eq!(headers1.len(), 4);
    assert_eq!(headers1[0].name, b":status");
    assert_eq!(headers1[0].value, b"302");
    assert_eq!(headers1[1].name, b"cache-control");
    assert_eq!(headers1[1].value, b"private");
    assert_eq!(headers1[2].name, b"date");
    assert_eq!(headers1[2].value, b"Mon, 21 Oct 2013 20:13:21 GMT");
    assert_eq!(headers1[3].name, b"location");
    assert_eq!(headers1[3].value, b"https://www.example.com");
}

/// Huffman encoding test for "www.example.com"
#[test]
fn test_huffman_www_example_com() {
    let input = b"www.example.com";
    let encoded = shiguredo_http2::hpack::huffman::encode_to_vec(input);

    // RFC 7541 の例では 12 バイト
    assert_eq!(encoded.len(), 12);

    // デコードして元に戻ることを確認
    let decoded = shiguredo_http2::hpack::huffman::decode(&encoded).unwrap();
    assert_eq!(decoded, input);
}

/// エンコーダーとデコーダーの往復テスト
#[test]
fn test_encoder_decoder_roundtrip() {
    let headers = vec![
        HeaderField::from_str(":method", "GET"),
        HeaderField::from_str(":scheme", "https"),
        HeaderField::from_str(":path", "/index.html"),
        HeaderField::from_str(":authority", "www.example.com"),
        HeaderField::from_str("accept", "text/html"),
        HeaderField::from_str("accept-encoding", "gzip, deflate"),
    ];

    let mut encoder = HpackEncoder::new(4096);
    let mut decoder = HpackDecoder::new(4096);

    let mut encoded = Vec::new();
    encoder.encode(&mut encoded, &headers);

    let decoded = decoder.decode(&encoded).unwrap();

    assert_eq!(decoded.len(), headers.len());
    for (orig, dec) in headers.iter().zip(decoded.iter()) {
        assert_eq!(orig.name, dec.name);
        assert_eq!(orig.value, dec.value);
    }
}

/// 複数リクエストでの動的テーブル活用テスト
#[test]
fn test_multiple_requests_dynamic_table() {
    let mut encoder = HpackEncoder::new(4096);
    let mut decoder = HpackDecoder::new(4096);

    // 最初のリクエスト
    let headers1 = vec![
        HeaderField::from_str(":method", "GET"),
        HeaderField::from_str(":path", "/"),
        HeaderField::from_str("custom-header", "value1"),
    ];

    let mut encoded1 = Vec::new();
    encoder.encode(&mut encoded1, &headers1);

    let decoded1 = decoder.decode(&encoded1).unwrap();
    assert_eq!(decoded1.len(), 3);

    // 2番目のリクエスト（同じカスタムヘッダー名を使用）
    let headers2 = vec![
        HeaderField::from_str(":method", "GET"),
        HeaderField::from_str(":path", "/other"),
        HeaderField::from_str("custom-header", "value2"),
    ];

    let mut encoded2 = Vec::new();
    encoder.encode(&mut encoded2, &headers2);

    let decoded2 = decoder.decode(&encoded2).unwrap();
    assert_eq!(decoded2.len(), 3);
    assert_eq!(decoded2[2].name, b"custom-header");
    assert_eq!(decoded2[2].value, b"value2");

    // 2番目のリクエストは動的テーブルを活用するため、より短くなるはず
    // （ただし、これは最適化の度合いによる）
}
