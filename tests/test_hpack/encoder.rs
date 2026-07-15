use shiguredo_http2::hpack::{Encoder, HeaderField};

#[test]
fn test_encode_header_list() {
    let mut encoder = Encoder::new(4096);
    let mut buf = Vec::new();

    let headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new(":path", "/").expect("valid header field"),
    ];

    encoder.encode(&mut buf, &headers);

    // :method: GET should be indexed (0x82)
    // :path: / should be indexed (0x84)
    assert!(buf.contains(&0x82));
    assert!(buf.contains(&0x84));
}

#[test]
fn test_encode_sensitive_header() {
    let mut encoder = Encoder::new(4096);
    let mut buf = Vec::new();

    let headers = vec![
        HeaderField::new(":method", "GET").expect("valid header field"),
        HeaderField::new_with_sensitive("authorization", "Bearer token", true)
            .expect("valid header field"),
    ];

    encoder.encode(&mut buf, &headers);

    // :method: GET should be indexed (0x82)
    assert_eq!(buf[0], 0x82);
    // authorization should be Never Indexed (0x1x prefix)
    assert_eq!(buf[1] & 0xF0, 0x10);
}
