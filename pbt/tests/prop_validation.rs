//! HTTP セマンティクス検証の PBT

use proptest::prelude::*;
use shiguredo_http2::{HeaderField, validation};

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
            HeaderField::from_str(":method", method),
            HeaderField::from_str(":scheme", scheme),
            HeaderField::from_str(":path", &path),
            HeaderField::from_str(":authority", &authority),
        ];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "te"]
                .contains(&name_str.as_ref())
            {
                headers.push(HeaderField::new(name, value));
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
        let mut headers = vec![HeaderField::from_str(":status", &status)];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "te"]
                .contains(&name_str.as_ref())
            {
                headers.push(HeaderField::new(name, value));
            }
        }

        prop_assert!(validation::validate_response_headers(&headers).is_ok());
    }

    /// 有効な CONNECT リクエストは検証を通過する
    #[test]
    fn prop_valid_connect_passes(
        authority in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}:[0-9]{1,5}",
        regular_headers in prop::collection::vec(
            (valid_header_name(), valid_header_value()),
            0..=4
        ),
    ) {
        let mut headers = vec![
            HeaderField::from_str(":method", "CONNECT"),
            HeaderField::from_str(":authority", &authority),
        ];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "te"]
                .contains(&name_str.as_ref())
            {
                headers.push(HeaderField::new(name, value));
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::new(forbidden.as_bytes().to_vec(), value),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 大文字を含むヘッダー名は拒否される
    #[test]
    fn prop_uppercase_header_name_rejected(
        prefix in "[a-z]{1,8}",
        suffix in "[a-z]{1,8}",
    ) {
        let name = format!("{prefix}X{suffix}"); // 大文字 X を挿入
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(&name, "value"),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 疑似ヘッダーの重複は拒否される
    #[test]
    fn prop_duplicate_pseudo_header_rejected(
        pseudo in prop_oneof![
            Just(":method"),
            Just(":scheme"),
            Just(":path"),
        ],
    ) {
        let mut headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
        ];

        // 重複を追加（:method, :scheme, :path のいずれか）
        headers.push(HeaderField::from_str(pseudo, "duplicate"));

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// 通常ヘッダー後の疑似ヘッダーは拒否される
    #[test]
    fn prop_pseudo_after_regular_rejected(
        extra_pseudo in prop_oneof![
            Just(":authority"),
        ],
    ) {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str("content-type", "text/html"),
            HeaderField::from_str(extra_pseudo, "value"),
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
                if ["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "te"]
                    .contains(&name_str.as_ref())
                {
                    None
                } else {
                    Some(HeaderField::new(name, value))
                }
            })
            .collect();

        prop_assert!(validation::validate_trailers(&headers).is_ok());
    }

    /// トレーラーに疑似ヘッダーがあると拒否される
    #[test]
    fn prop_trailers_with_pseudo_rejected(
        pseudo in prop_oneof![
            Just(":status"),
            Just(":method"),
            Just(":path"),
            Just(":scheme"),
            Just(":authority"),
        ],
    ) {
        let headers = vec![
            HeaderField::from_str(pseudo, "value"),
            HeaderField::from_str("x-trailer", "value"),
        ];

        prop_assert!(validation::validate_trailers(&headers).is_err());
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
            HeaderField::from_str(":method", "CONNECT"),
            HeaderField::from_str(":scheme", scheme),
            HeaderField::from_str(":path", &path),
            HeaderField::from_str(":authority", &authority),
            HeaderField::from_str(":protocol", protocol),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// Extended CONNECT に :scheme がない場合は拒否される
    #[test]
    fn prop_extended_connect_without_scheme_rejected(
        path in http_path(),
        authority in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}:[0-9]{1,5}",
    ) {
        let headers = vec![
            HeaderField::from_str(":method", "CONNECT"),
            HeaderField::from_str(":path", &path),
            HeaderField::from_str(":authority", &authority),
            HeaderField::from_str(":protocol", "webtransport"),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// Extended CONNECT に :path がない場合は拒否される
    #[test]
    fn prop_extended_connect_without_path_rejected(
        scheme in http_scheme(),
        authority in "[a-z][a-z0-9]{0,10}\\.[a-z]{2,3}:[0-9]{1,5}",
    ) {
        let headers = vec![
            HeaderField::from_str(":method", "CONNECT"),
            HeaderField::from_str(":scheme", scheme),
            HeaderField::from_str(":authority", &authority),
            HeaderField::from_str(":protocol", "webtransport"),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", scheme),
            HeaderField::from_str(":path", &path),
            HeaderField::from_str(":authority", &authority),
            HeaderField::from_str("host", &host),
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", scheme),
            HeaderField::from_str(":path", &path),
            HeaderField::from_str(":authority", &authority),
            HeaderField::from_str("host", &authority),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// CONNECT 以外で :protocol を使うと拒否される
    #[test]
    fn prop_protocol_on_non_connect_rejected(
        method in prop_oneof![
            Just("GET"),
            Just("POST"),
            Just("PUT"),
            Just("DELETE"),
        ],
        scheme in http_scheme(),
        path in http_path(),
    ) {
        let headers = vec![
            HeaderField::from_str(":method", method),
            HeaderField::from_str(":scheme", scheme),
            HeaderField::from_str(":path", &path),
            HeaderField::from_str(":protocol", "webtransport"),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// http/https スキームで :authority も Host もない場合は拒否される (RFC 9113 Section 8.3.1)
    #[test]
    fn prop_http_request_without_authority_or_host_rejected(
        method in http_method(),
        scheme in http_scheme(),
        path in http_path(),
    ) {
        let headers = vec![
            HeaderField::from_str(":method", method),
            HeaderField::from_str(":scheme", scheme),
            HeaderField::from_str(":path", &path),
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
            HeaderField::from_str(":method", method),
            HeaderField::from_str(":scheme", scheme),
            HeaderField::from_str(":path", &path),
            HeaderField::from_str("host", &host),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }

    /// 制御文字、スペース、デリミタを含むヘッダー名は拒否される (RFC 9110 Section 5.1)
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::new(name, b"value".to_vec()),
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::new(b"x-test".to_vec(), value),
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::new(b"x-test".to_vec(), value),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_err());
    }

    /// :status = "101" のレスポンスは拒否される (RFC 9113 Section 8.3.2)
    #[test]
    fn prop_response_status_101_rejected(
        regular_headers in prop::collection::vec(
            (valid_header_name(), valid_header_value()),
            0..=4
        ),
    ) {
        let mut headers = vec![HeaderField::from_str(":status", "101")];

        for (name, value) in regular_headers {
            let name_str = String::from_utf8_lossy(&name);
            if !["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "te"]
                .contains(&name_str.as_ref())
            {
                headers.push(HeaderField::new(name, value));
            }
        }

        prop_assert!(validation::validate_response_headers(&headers).is_err());
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::new(b"x-test".to_vec(), value),
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::new(b"x-test".to_vec(), value),
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
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::new(b"x-test".to_vec(), value),
        ];

        prop_assert!(validation::validate_request_headers(&headers).is_ok());
    }
}
