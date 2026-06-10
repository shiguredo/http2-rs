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

// RFC 9113 Section 8.3.1: 全ての HTTP/2 リクエストは :method/:scheme/:path をちょうど 1 つずつ含まなければならない (MUST)。欠落は malformed。
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

// RFC 9113 Section 8.3: 同一の擬似ヘッダー名は field block 内に 2 回以上現れてはならない (MUST NOT)。
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

// RFC 9113 Section 8.3: :scheme の重複も拒否される
#[test]
fn test_duplicate_scheme() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":scheme", "http"),
        h(":path", "/"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 9113 Section 8.3: :path の重複も拒否される
#[test]
fn test_duplicate_path() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h(":path", "/dup"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 9113 Section 8.3: 擬似ヘッダーは全ての通常フィールドより前に現れなければならない (MUST)。違反は malformed。
#[test]
fn test_pseudo_header_after_regular() {
    let headers = vec![
        h(":method", "GET"),
        h("content-type", "text/html"),
        h(":scheme", "https"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 9113 Section 8.3: 通常ヘッダーの後ろに :authority が現れた場合も拒否される
#[test]
fn test_pseudo_authority_after_regular() {
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        h("content-type", "text/html"),
        h(":authority", "example.com"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 9113 Section 8.2.2: Connection や Transfer-Encoding 等の接続固有ヘッダーを含むメッセージは malformed として扱わなければならない (MUST)。
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

// RFC 9113 Section 8.2.2: TE は "trailers" 以外の値を含んではならない (MUST NOT)。
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
    // RFC 9113 Section 8.3.1: "http"/"https" URI で :path は空であってはならない (MUST NOT)。
    let headers = vec![h(":method", "GET"), h(":scheme", "https"), h(":path", "")];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 9113 Section 8.5: CONNECT リクエストでは :scheme と :path を省略しなければならない (MUST)。
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

// RFC 9113 Section 8.3.2: :status は全てのレスポンスに含めなければならない (MUST)。
#[test]
fn test_response_missing_status() {
    let headers = vec![h("content-type", "text/html")];
    assert!(validate_response_headers(&headers).is_err());
}

// RFC 9113 Section 8.3: リクエスト用に定義された擬似ヘッダーはレスポンスに現れてはならない (MUST NOT)。
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

// RFC 9113 Section 8.3.1: サーバーは :authority と異なる Host を含むリクエストを malformed として扱うべき (SHOULD)。
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

// RFC 9113 Section 8.3: 擬似ヘッダーはトレーラーセクションに現れてはならない (MUST NOT)。
#[test]
fn test_trailers_with_pseudo_header() {
    let headers = vec![h(":status", "200"), h("x-trailer", "value")];
    assert!(validate_trailers(&headers).is_err());
}

// RFC 9113 Section 8.3: トレーラーに :method が含まれる場合も拒否される
#[test]
fn test_trailers_with_pseudo_method() {
    let headers = vec![h(":method", "GET"), h("x-trailer", "value")];
    assert!(validate_trailers(&headers).is_err());
}

// RFC 9113 Section 8.3: トレーラーに :path が含まれる場合も拒否される
#[test]
fn test_trailers_with_pseudo_path() {
    let headers = vec![h(":path", "/"), h("x-trailer", "value")];
    assert!(validate_trailers(&headers).is_err());
}

// RFC 9113 Section 8.3: トレーラーに :scheme が含まれる場合も拒否される
#[test]
fn test_trailers_with_pseudo_scheme() {
    let headers = vec![h(":scheme", "https"), h("x-trailer", "value")];
    assert!(validate_trailers(&headers).is_err());
}

// RFC 9113 Section 8.3: トレーラーに :authority が含まれる場合も拒否される
#[test]
fn test_trailers_with_pseudo_authority() {
    let headers = vec![h(":authority", "example.com"), h("x-trailer", "value")];
    assert!(validate_trailers(&headers).is_err());
}

#[test]
fn test_response_status_101_disallowed() {
    // HeaderField::new は 101 を通す (3DIGIT 検査のみ)。
    // RFC 9113 Section 8.6: HTTP/2 は 101 (Switching Protocols) をサポートしないため validation 側で弾く。
    let headers = vec![h(":status", "101")];
    assert!(validate_response_headers(&headers).is_err());
}

// RFC 8441 Section 4: Extended CONNECT に :scheme がない場合は拒否される
// (:protocol を含むリクエストには :scheme と :path が必須)。
#[test]
fn test_extended_connect_without_scheme_rejected() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":path", "/"),
        h(":authority", "example.com:443"),
        h(":protocol", "webtransport"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 8441 Section 4: Extended CONNECT に :path がない場合は拒否される。
#[test]
fn test_extended_connect_without_path_rejected() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":scheme", "https"),
        h(":authority", "example.com:443"),
        h(":protocol", "webtransport"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 8441 Section 4: CONNECT 以外のメソッドで :protocol を使うと拒否される。
#[test]
fn test_protocol_on_non_connect_rejected() {
    for method in ["GET", "POST", "PUT", "DELETE"] {
        let headers = vec![
            h(":method", method),
            h(":scheme", "https"),
            h(":path", "/"),
            h(":protocol", "webtransport"),
        ];
        assert!(
            validate_request_headers(&headers).is_err(),
            "method={method} は :protocol を含むときに拒否されるべき"
        );
    }
}

// RFC 9113 Section 8.5: CONNECT の :authority にポートがない場合は拒否される
// (authority-form は host:port を要求する)。
#[test]
fn test_connect_authority_without_port_rejected() {
    let headers = vec![h(":method", "CONNECT"), h(":authority", "example.com")];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 9113 Section 8.5: CONNECT の :authority が IPv6 の authority-form なら通過する。
#[test]
fn test_connect_ipv6_authority_accepted() {
    let headers = vec![h(":method", "CONNECT"), h(":authority", "[::1]:443")];
    assert!(validate_request_headers(&headers).is_ok());
}

// RFC 9113 Section 8.3.1: OPTIONS 以外のメソッドで :path = "*" は拒否される。
#[test]
fn test_asterisk_path_on_non_options_rejected() {
    for method in ["GET", "POST", "PUT", "DELETE", "HEAD", "PATCH"] {
        let headers = vec![
            h(":method", method),
            h(":scheme", "https"),
            h(":path", "*"),
            h(":authority", "example.com"),
        ];
        assert!(
            validate_request_headers(&headers).is_err(),
            "method={method} で :path = * は拒否されるべき"
        );
    }
}

// RFC 9113 Section 8.3.1: OPTIONS で :path = "*" は通過する。
#[test]
fn test_asterisk_path_on_options_accepted() {
    let headers = vec![
        h(":method", "OPTIONS"),
        h(":scheme", "https"),
        h(":path", "*"),
        h(":authority", "example.com"),
    ];
    assert!(validate_request_headers(&headers).is_ok());
}
