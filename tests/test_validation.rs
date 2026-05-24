use shiguredo_http2::hpack::HeaderField;
use shiguredo_http2::validation::{
    validate_request_headers, validate_response_headers, validate_trailers,
};

fn h(name: &str, value: &str) -> HeaderField {
    HeaderField::new(name, value).unwrap()
}

#[test]
fn test_valid_get_request() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h(":authority", "example.com"),
    ];

    assert!(validate_request_headers(&headers).is_ok());
}

#[test]
fn test_valid_connect_request() {
    let headers = vec![h(":method", "CONNECT"), h(":authority", "example.com:443")];
    assert!(validate_request_headers(&headers).is_ok());
}

#[test]
fn test_missing_method() {
    let headers = vec![h(":scheme", "https"), h(":path", "/")];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_missing_scheme() {
    let headers = vec![h(":method", "GET"), h(":path", "/")];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_missing_path() {
    let headers = vec![h(":method", "GET"), h(":scheme", "https")];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_duplicate_method() {
    let headers = vec![
        h(":method", "GET"),
        h(":method", "POST"),
        h(":scheme", "https"),
        h(":path", "/"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_pseudo_header_after_regular() {
    let headers = vec![
        h(":method", "GET"),
        h("content-type", "text/html"),
        h(":scheme", "https"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_forbidden_connection_header() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h("connection", "close"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_forbidden_transfer_encoding() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h("transfer-encoding", "chunked"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_te_trailers_allowed() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h(":authority", "example.com"),
        h("te", "trailers"),
    ];
    assert!(validate_request_headers(&headers).is_ok());
}

#[test]
fn test_te_gzip_forbidden() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h("te", "gzip"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_te_trailers_forbidden_in_response() {
    // RFC 9113 §8.2.2: TE ヘッダーの例外はリクエストに限定される
    let headers = vec![h(":status", "200"), h("te", "trailers")];
    assert!(validate_response_headers(&headers).is_err());
}

#[test]
fn test_te_trailers_forbidden_in_trailers() {
    let headers = vec![h("te", "trailers")];
    assert!(validate_trailers(&headers).is_err());
}

#[test]
fn test_empty_path() {
    // 空 :path は HeaderField::new では通る (scheme 依存判定のため)
    let headers = vec![h(":method", "GET"), h(":scheme", "https"), h(":path", "")];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_connect_with_path() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":authority", "example.com:443"),
        h(":path", "/"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_valid_response() {
    let headers = vec![h(":status", "200"), h("content-type", "text/html")];
    assert!(validate_response_headers(&headers).is_ok());
}

#[test]
fn test_response_missing_status() {
    let headers = vec![h("content-type", "text/html")];
    assert!(validate_response_headers(&headers).is_err());
}

#[test]
fn test_response_with_method() {
    let headers = vec![h(":status", "200"), h(":method", "GET")];
    assert!(validate_response_headers(&headers).is_err());
}

#[test]
fn test_valid_trailers() {
    let headers = vec![h("x-checksum", "abc123"), h("x-trailer", "value")];
    assert!(validate_trailers(&headers).is_ok());
}

#[test]
fn test_host_authority_mismatch() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h(":authority", "example.com"),
        h("host", "other.com"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_host_authority_match() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h(":authority", "example.com"),
        h("host", "example.com"),
    ];
    assert!(validate_request_headers(&headers).is_ok());
}

#[test]
fn test_trailers_with_pseudo_header() {
    let headers = vec![h(":status", "200"), h("x-trailer", "value")];
    assert!(validate_trailers(&headers).is_err());
}

#[test]
fn test_response_status_101_disallowed() {
    // HeaderField::new は 101 を通す (3DIGIT 検査のみ)。
    // 101 は HTTP/2 でサポートされないため validation 側で弾く。
    let headers = vec![h(":status", "101")];
    assert!(validate_response_headers(&headers).is_err());
}
