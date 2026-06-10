//! HTTP セマンティクス検証の PBT

use proptest::prelude::*;
use shiguredo_http2::{HeaderField, validation};

/// PBT で生成した「ランダム通常ヘッダー」が以下の場合は除外する:
/// - リクエストで禁止 (RFC 9113 §8.2.2): connection / keep-alive / proxy-connection
///   / transfer-encoding / upgrade
/// - te は §8.2.2 の例外として許可される (MAY) が "trailers" 以外の値は禁止のため、
///   ランダム値生成では除外する
/// - PBT で固定 authority と不一致を生む可能性がある: host
const FORBIDDEN_FOR_VALIDATION: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "upgrade",
    "te",
    "host",
];

/// 有効なヘッダー名を生成する（小文字 ASCII）
fn valid_header_name() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        prop::sample::select(
            (b'a'..=b'z')
                .chain(b'0'..=b'9')
                .chain([b'-', b'_'])
                .collect::<Vec<_>>(),
        ),
        1..=16,
    )
}

/// 有効なヘッダー値を生成する（印字可能 ASCII）
///
/// RFC 9113 Section 8.2.1: 先頭/末尾の SP (0x20) / HTAB (0x09) は禁止。
/// 空値または先頭/末尾が SP/HTAB でない値を生成する。
fn valid_header_value() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0x20u8..=0x7Eu8, 0..=32).prop_map(|mut v| {
        // 先頭の SP/HTAB を除去
        while v.first().is_some_and(|&b| b == 0x20 || b == 0x09) {
            v.remove(0);
        }
        // 末尾の SP/HTAB を除去
        while v.last().is_some_and(|&b| b == 0x20 || b == 0x09) {
            v.pop();
        }
        v
    })
}

/// HTTP メソッドを生成する
fn http_method() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("GET"),
        Just("POST"),
        Just("PUT"),
        Just("DELETE"),
        Just("HEAD"),
        Just("OPTIONS"),
        Just("PATCH"),
    ]
}

/// HTTP スキームを生成する
fn http_scheme() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just("http"), Just("https"),]
}

/// 有効な HTTP パスを生成する
fn http_path() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(
            (b'a'..=b'z')
                .chain(b'0'..=b'9')
                .chain([b'/', b'-', b'_', b'.'])
                .collect::<Vec<_>>(),
        ),
        1..=32,
    )
    .prop_map(|bytes| {
        let mut path = String::from("/");
        for b in bytes {
            path.push(b as char);
        }
        path
    })
}

/// HTTP ステータスコードを生成する
fn http_status() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("200".to_string()),
        Just("201".to_string()),
        Just("204".to_string()),
        Just("301".to_string()),
        Just("302".to_string()),
        Just("400".to_string()),
        Just("401".to_string()),
        Just("403".to_string()),
        Just("404".to_string()),
        Just("500".to_string()),
        Just("502".to_string()),
        Just("503".to_string()),
    ]
}

proptest! {
    /// 有効なリクエストヘッダーは検証を通過する
    #[test]
    fn prop_valid_request_passes(
        method in http_method(),
        scheme in http_scheme(),
        path in http_path(),
        authority in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}",
        regular_headers in prop::collection::vec(
            (valid_header_name(), valid_header_value()),
            0..=4
        ),
    ) {
        let mut headers = vec![
            HeaderField::new(":method", method).unwrap(),
            HeaderField::new(":scheme", scheme).unwrap(),
            HeaderField::new(":path", &path).unwrap(),
            HeaderField::new(":authority", &authority).unwrap(),
        ];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !FORBIDDEN_FOR_VALIDATION
                .contains(&name_str.as_ref())
            {
                headers.push(HeaderField::new(name, value).unwrap());
            }
        }

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// 有効なレスポンスヘッダーは検証を通過する
    #[test]
    fn prop_valid_response_passes(
        status in http_status(),
        regular_headers in prop::collection::vec(
            (valid_header_name(), valid_header_value()),
            0..=4
        ),
    ) {
        let mut headers = vec![HeaderField::new(":status", &status).unwrap()];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !FORBIDDEN_FOR_VALIDATION
                .contains(&name_str.as_ref())
            {
                headers.push(HeaderField::new(name, value).unwrap());
            }
        }

        prop_assert!(validation::validate_response_headers(&headers).is_ok());
    }

    /// 有効な CONNECT リクエストは検証を通過する
    #[test]
    fn prop_valid_connect_passes(
        authority in ("[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}", 1u16..=65535u16)
            .prop_map(|(host, port)| format!("{host}:{port}")),
        regular_headers in prop::collection::vec(
            (valid_header_name(), valid_header_value()),
            0..=4
        ),
    ) {
        let mut headers = vec![
            HeaderField::new(":method", "CONNECT").unwrap(),
            HeaderField::new(":authority", &authority).unwrap(),
        ];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !FORBIDDEN_FOR_VALIDATION
                .contains(&name_str.as_ref())
            {
                headers.push(HeaderField::new(name, value).unwrap());
            }
        }

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// 禁止ヘッダーは拒否される
    #[test]
    fn prop_forbidden_headers_rejected(
        forbidden in prop_oneof![
            Just("connection"),
            Just("keep-alive"),
            Just("proxy-connection"),
            Just("transfer-encoding"),
            Just("upgrade"),
        ],
        value in valid_header_value(),
    ) {
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(forbidden.as_bytes(), value).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 大文字を含むヘッダー名は拒否される (RFC 9113 Section 8.2.1)
    #[test]
    fn prop_uppercase_header_name_rejected(
        prefix in "[a-z]{1,8}",
        suffix in "[a-z]{1,8}",
    ) {
        let name = format!("{prefix}X{suffix}"); // 大文字 X を挿入
        // HeaderField::new は構築時に弾くため、validation 経路の検査を確認するには
        // wire 上の不正データを模擬する from_validated_parts で構築する。
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            pbt::wire_header_field(&name.into_bytes(), b"value"),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 有効なトレーラーは検証を通過する
    #[test]
    fn prop_valid_trailers_passes(
        trailers in prop::collection::vec(
            (valid_header_name(), valid_header_value()),
            0..=4
        ),
    ) {
        let headers: Vec<HeaderField> = trailers
            .into_iter()
            .filter_map(|(name, value)| {
                // 禁止ヘッダーを避ける
                let name_str = String::from_utf8_lossy(&name);
                if FORBIDDEN_FOR_VALIDATION
                    .contains(&name_str.as_ref())
                {
                    None
                } else {
                    Some(HeaderField::new(name, value).unwrap())
                }
            })
            .collect();

        prop_assert!(validation::validate_trailers(&headers).is_ok());
    }

    /// 有効な Extended CONNECT リクエストは検証を通過する (RFC 8441)
    #[test]
    fn prop_valid_extended_connect_passes(
        scheme in http_scheme(),
        path in http_path(),
        authority in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}:[0-9]{1,5}",
        protocol in prop_oneof![
            Just("webtransport"),
            Just("websocket"),
        ],
    ) {
        let headers = vec![
            HeaderField::new(":method", "CONNECT").unwrap(),
            HeaderField::new(":scheme", scheme).unwrap(),
            HeaderField::new(":path", &path).unwrap(),
            HeaderField::new(":authority", &authority).unwrap(),
            HeaderField::new(":protocol", protocol).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// Host と :authority が不一致の場合は拒否される (RFC 9113 Section 8.3.1)
    #[test]
    fn prop_host_authority_mismatch_rejected(
        authority in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}",
        host_suffix in "[a-z]{1,5}",
        scheme in http_scheme(),
        path in http_path(),
    ) {
        let host = format!("{authority}.{host_suffix}");
        prop_assume!(authority != host);
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", scheme).unwrap(),
            HeaderField::new(":path", &path).unwrap(),
            HeaderField::new(":authority", &authority).unwrap(),
            HeaderField::new("host", &host).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// Host と :authority が一致する場合は通過する (RFC 9113 Section 8.3.1)
    #[test]
    fn prop_host_authority_match_accepted(
        authority in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}",
        scheme in http_scheme(),
        path in http_path(),
    ) {
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", scheme).unwrap(),
            HeaderField::new(":path", &path).unwrap(),
            HeaderField::new(":authority", &authority).unwrap(),
            HeaderField::new("host", &authority).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// http/https スキームで :authority も Host もない場合は拒否される (RFC 9113 Section 8.3.1)
    #[test]
    fn prop_http_request_without_authority_or_host_rejected(
        method in http_method(),
        scheme in http_scheme(),
        path in http_path(),
    ) {
        let headers = vec![
            HeaderField::new(":method", method).unwrap(),
            HeaderField::new(":scheme", scheme).unwrap(),
            HeaderField::new(":path", &path).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// :authority がなくても Host があれば通過する (RFC 9113 Section 8.3.1)
    #[test]
    fn prop_http_request_with_host_only_passes(
        method in http_method(),
        scheme in http_scheme(),
        path in http_path(),
        host in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}",
    ) {
        let headers = vec![
            HeaderField::new(":method", method).unwrap(),
            HeaderField::new(":scheme", scheme).unwrap(),
            HeaderField::new(":path", &path).unwrap(),
            HeaderField::new("host", &host).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// 制御文字、スペース、デリミタ、大文字を含むヘッダー名は拒否される (RFC 9110 Section 5.6.2 / RFC 9113 Section 8.2.1)
    #[test]
    fn prop_header_name_with_invalid_chars_rejected(
        prefix in "[a-z]{1,4}",
        invalid_char in prop::sample::select(vec![
            0x00u8, 0x01, 0x09, 0x20, b'(', b')', b'<', b'>', b'@',
            b',', b';', b':', b'\\', b'"', b'/', b'[', b']', b'?', b'=',
            b'{', b'}', b'A', b'Z',
        ]),
        suffix in "[a-z]{1,4}",
    ) {
        let mut name = prefix.into_bytes();
        name.push(invalid_char);
        name.extend_from_slice(suffix.as_bytes());

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
            // HeaderField::new は構築時に弾くため、wire 由来データを模擬するために
            // from_validated_parts を使って validation 経路の検査を確認する。
            pbt::wire_header_field(&name, b"value"),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// NUL を含むヘッダー値は拒否される (RFC 9110 Section 5.5)
    #[test]
    fn prop_header_value_with_nul_rejected(
        prefix in "[a-z]{0,4}",
        suffix in "[a-z]{0,4}",
    ) {
        let mut value = prefix.into_bytes();
        value.push(0x00);
        value.extend_from_slice(suffix.as_bytes());

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
            pbt::wire_header_field(b"x-test", &value),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// CR/LF を含むヘッダー値は拒否される (RFC 9110 Section 5.5)
    #[test]
    fn prop_header_value_with_cr_lf_rejected(
        prefix in "[a-z]{0,4}",
        bad_byte in prop::sample::select(vec![0x0du8, 0x0a]),
        suffix in "[a-z]{0,4}",
    ) {
        let mut value = prefix.into_bytes();
        value.push(bad_byte);
        value.extend_from_slice(suffix.as_bytes());

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
            pbt::wire_header_field(b"x-test", &value),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 先頭に SP/HTAB があるヘッダー値は拒否される (RFC 9113 Section 8.2.1)
    #[test]
    fn prop_header_value_leading_whitespace_rejected(
        ws in prop::sample::select(vec![0x20u8, 0x09]),
        body in "[a-z]{1,8}",
    ) {
        let mut value = vec![ws];
        value.extend_from_slice(body.as_bytes());

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
            pbt::wire_header_field(b"x-test", &value),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 末尾に SP/HTAB があるヘッダー値は拒否される (RFC 9113 Section 8.2.1)
    #[test]
    fn prop_header_value_trailing_whitespace_rejected(
        body in "[a-z]{1,8}",
        ws in prop::sample::select(vec![0x20u8, 0x09]),
    ) {
        let mut value = body.into_bytes();
        value.push(ws);

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
            pbt::wire_header_field(b"x-test", &value),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 内部の SP はヘッダー値として許可される (RFC 9113 Section 8.2.1)
    #[test]
    fn prop_header_value_internal_space_accepted(
        prefix in "[a-z]{1,8}",
        suffix in "[a-z]{1,8}",
    ) {
        let value = format!("{prefix} {suffix}").into_bytes();

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "https").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", "example.com").unwrap(),
            pbt::wire_header_field(b"x-test", &value),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// CONNECT の :authority が authority-form (host:port) なら通過する (RFC 9113 Section 8.5)
    #[test]
    fn prop_connect_authority_with_port_accepted(
        host in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}",
        port in 1u16..=65535u16,
    ) {
        let authority = format!("{host}:{port}");
        let headers = vec![
            HeaderField::new(":method", "CONNECT").unwrap(),
            HeaderField::new(":authority", &authority).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// http/https スキームで :authority に userinfo (@) がある場合は拒否される (RFC 9113 Section 8.3.1)
    #[test]
    fn prop_userinfo_in_authority_rejected_for_http(
        user in "[a-z]{1,4}",
        host in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}",
        scheme in http_scheme(),
    ) {
        let authority = format!("{user}@{host}");
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", scheme).unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", &authority).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 非 HTTP スキームで :authority に userinfo (@) があっても通過する (RFC 9113 Section 8.3.1)
    #[test]
    fn prop_userinfo_in_authority_accepted_for_non_http(
        user in "[a-z]{1,4}",
        host in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}",
    ) {
        let authority = format!("{user}@{host}");
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", "ftp").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
            HeaderField::new(":authority", &authority).unwrap(),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// RFC 9113 §8.3.1: http/https (大文字小文字不問) で :path 空は EmptyPath エラーとなり、
    /// それ以外のスキームでは :path 空が許容されることを検証する
    #[test]
    fn prop_empty_path_scheme_dependent(
        scheme in prop_oneof![
            // Strategy A: http/https の大文字小文字異綴
            "[hH][tT][tT][pP]",
            "[hH][tT][tT][pP][sS]",
            // Strategy B: http/https 以外 (長さ 1-3, 6-8 で衝突を回避)
            "[a-zA-Z]",
            "[a-zA-Z][a-zA-Z0-9+\\-.]{1}",
            "[a-zA-Z][a-zA-Z0-9+\\-.]{2}",
            "[a-zA-Z][a-zA-Z0-9+\\-.]{5}",
            "[a-zA-Z][a-zA-Z0-9+\\-.]{6}",
            "[a-zA-Z][a-zA-Z0-9+\\-.]{7}",
        ],
    ) {
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":scheme", &scheme).unwrap(),
            HeaderField::new(":path", "").unwrap(),
        ];
        let result = validation::validate_request_headers(&headers);
        let is_http_or_https = scheme.eq_ignore_ascii_case("http")
            || scheme.eq_ignore_ascii_case("https");
        if is_http_or_https {
            prop_assert!(result.is_err(), "http/https スキームで :path 空は拒否されるべき: scheme={scheme}");
        } else {
            prop_assert!(result.is_ok(), "http/https 以外で :path 空は許容されるべき: scheme={scheme}");
        }
    }
}
