use shiguredo_http2::hpack::HeaderField;
use shiguredo_http2::validation::{
    validate_request_headers, validate_response_headers, validate_trailers,
};
use shiguredo_http2::{HpackDecoder, HpackEncoder};

fn h(name: &str, value: &str) -> HeaderField {
    HeaderField::new(name, value).expect("テスト用ヘッダーは有効である")
}

/// HPACK decoder 経路で構築された HeaderField を返す (wire 上のバイト列を模擬)
fn wire_header_field(name: &[u8], value: &[u8]) -> HeaderField {
    let mut encoder = HpackEncoder::new(4096);
    encoder.set_huffman(false);
    let mut wire = Vec::new();
    encoder.encode_header(&mut wire, name, value, false);
    let mut decoder = HpackDecoder::new(4096);
    let headers = decoder
        .decode(&wire)
        .expect("wire 符号化は有効な HPACK である");
    headers
        .into_iter()
        .next()
        .expect("wire 符号化は 1 件のヘッダーを生成する")
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

// draft-ietf-webtrans-http2-15 Section 3.2: :protocol=webtransport + :scheme=http は拒否される
#[test]
fn test_webtransport_scheme_http_rejected() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":scheme", "http"),
        h(":path", "/"),
        h(":authority", "example.com:443"),
        h(":protocol", "webtransport"),
    ];
    assert!(
        validate_request_headers(&headers).is_err(),
        ":protocol=webtransport + :scheme=http は拒否されるべき"
    );
}

// draft-ietf-webtrans-http2-15 Section 3.2: :protocol=webtransport + :scheme=https は通過する
#[test]
fn test_webtransport_scheme_https_accepted() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":scheme", "https"),
        h(":path", "/"),
        h(":authority", "example.com:443"),
        h(":protocol", "webtransport"),
    ];
    assert!(
        validate_request_headers(&headers).is_ok(),
        ":protocol=webtransport + :scheme=https は通過すべき"
    );
}

// RFC 3986 Section 3.1: scheme は case-insensitive。大文字 HTTPS も通過する
#[test]
fn test_webtransport_scheme_https_uppercase_accepted() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":scheme", "HTTPS"),
        h(":path", "/"),
        h(":authority", "example.com:443"),
        h(":protocol", "webtransport"),
    ];
    assert!(
        validate_request_headers(&headers).is_ok(),
        ":protocol=webtransport + :scheme=HTTPS (大文字) は通過すべき"
    );
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

// RFC 9112 Section 3.2.3 / RFC 3986 Section 3.2.2: CONNECT の :authority の host 部に
// SP を含む値は拒否される (uri-host は SP を許さない)。
#[test]
fn test_connect_authority_with_space_in_host_rejected() {
    let headers = vec![h(":method", "CONNECT"), h(":authority", "foo bar:80")];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 3986 Section 3.2.2: host 部に制御文字を含む値は拒否される
// (validate_field_value を通過する 0x01 / 0x7f / 値内部 HTAB で検証する)。
#[test]
fn test_connect_authority_with_control_char_in_host_rejected() {
    for value in ["foo\u{1}bar:80", "foo\u{7f}bar:80", "foo\tbar:80"] {
        let headers = vec![h(":method", "CONNECT"), h(":authority", value)];
        assert!(
            validate_request_headers(&headers).is_err(),
            "制御文字入り host は拒否されるはず: {value:?}"
        );
    }
}

// RFC 3986 Section 3.2.2: host 部に非 ASCII バイトを含む値は拒否される。
#[test]
fn test_connect_authority_with_non_ascii_host_rejected() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":authority", "foo\u{00e9}bar:80"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 3986 Section 3.2.2: uri-host に許可されない文字を含む値は拒否される。
#[test]
fn test_connect_authority_with_invalid_host_char_rejected() {
    for value in [
        "foo/bar:80",
        "foo?bar:80",
        "foo#bar:80",
        "foo{bar}:80",
        "foo|bar:80",
        "foo\\bar:80",
        "foo^bar:80",
        "foo\"bar:80",
        "foo<bar>:80",
        "foo:bar:80",
        "foo%zz:80",
        "foo%:80",
        "foo%4:80",
        "foo%4g:80",
    ] {
        let headers = vec![h(":method", "CONNECT"), h(":authority", value)];
        assert!(
            validate_request_headers(&headers).is_err(),
            "不正文字入り host は拒否されるはず: {value:?}"
        );
    }
}

// RFC 3986 Section 3.2.2: IPv6 リテラル内部に不正文字を含む値は拒否される。
#[test]
fn test_connect_authority_with_invalid_ipv6_literal_rejected() {
    for value in ["[:: 1]:443", "[]:443", "[zz]:443", "[::g]:443"] {
        let headers = vec![h(":method", "CONNECT"), h(":authority", value)];
        assert!(
            validate_request_headers(&headers).is_err(),
            "不正な IPv6 リテラルは拒否されるはず: {value:?}"
        );
    }
}

// RFC 3986 Section 3.2.2: pct-encoded を含む host は受理される。
#[test]
fn test_connect_authority_with_pct_encoded_host_accepted() {
    let headers = vec![h(":method", "CONNECT"), h(":authority", "foo%20bar:80")];
    assert!(validate_request_headers(&headers).is_ok());
}

// RFC 3986 Section 3.2.2: unreserved / sub-delims を含む host は受理される。
#[test]
fn test_connect_authority_with_unreserved_and_sub_delims_accepted() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":authority", "a-b_c~d!e$f&g'h(i)j*k+l,m;n=o:80"),
    ];
    assert!(validate_request_headers(&headers).is_ok());
}

// RFC 3986 Section 3.2.3: ポート範囲外 (65535 超) は拒否される (既存挙動の回帰)。
#[test]
fn test_connect_authority_with_port_out_of_range_rejected() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":authority", "example.com:65536"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 9113 Section 8.3.1: CONNECT の :authority に userinfo (@) を含む値は拒否される
// (既存挙動の回帰)。
#[test]
fn test_connect_authority_with_userinfo_rejected() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":authority", "user@example.com:80"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

// authority-form は host を要求するため、空 host は拒否される。
#[test]
fn test_connect_authority_with_empty_host_rejected() {
    let headers = vec![h(":method", "CONNECT"), h(":authority", ":80")];
    assert!(validate_request_headers(&headers).is_err());
}

// RFC 3986 Section 3.2.2: 埋め込み IPv4 を含む IPv6 リテラルは受理される。
#[test]
fn test_connect_authority_with_ipv4_embedded_ipv6_accepted() {
    let headers = vec![
        h(":method", "CONNECT"),
        h(":authority", "[::ffff:192.168.0.1]:443"),
    ];
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

#[test]
fn test_uppercase_header_name_via_decoder_path() {
    // HPACK decoder 経路で構築された大文字 name は
    // check_field の再検査により InvalidHeaderField として弾かれる
    let headers = vec![
        h(":method", "GET"),
        h(":scheme", "https"),
        h(":path", "/"),
        wire_header_field(b"Content-Type", b"text/html"),
    ];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_empty_path_http_scheme_rejected() {
    // RFC 9113 §8.3.1: http スキームでは :path 空は malformed
    let headers = vec![h(":method", "GET"), h(":scheme", "http"), h(":path", "")];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_empty_path_https_scheme_rejected() {
    // RFC 9113 §8.3.1: https スキームでは :path 空は malformed
    let headers = vec![h(":method", "GET"), h(":scheme", "https"), h(":path", "")];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_empty_path_http_uppercase_rejected() {
    // eq_ignore_ascii_case で大文字 HTTP も拒否される
    let headers = vec![h(":method", "GET"), h(":scheme", "HTTP"), h(":path", "")];
    assert!(validate_request_headers(&headers).is_err());
}

#[test]
fn test_empty_path_non_http_scheme_accepted() {
    // http/https 以外のスキームでは :path 空は許容
    let headers = vec![h(":method", "GET"), h(":scheme", "ftp"), h(":path", "")];
    assert!(validate_request_headers(&headers).is_ok());
}
