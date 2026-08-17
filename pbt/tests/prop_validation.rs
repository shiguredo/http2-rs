//! HTTP セマンティクス検証の PBT

use shiguredo_http2::{HeaderField, validation};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

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

const LOWER_ALNUM_CHARSET: usize = 36; // a-z + 0-9
const LOWER_CHARSET: usize = 26; // a-z

// 小文字 ASCII (a-z) の char を生成する
fn sample_lower_ascii(ctx: &mut noprop::TestCaseContext) -> char {
    (b'a' + noprop::sample_usize_in(ctx, 0..LOWER_CHARSET) as u8) as char
}

/// 小文字 ASCII または数字 (a-z0-9) の char を生成する
fn sample_lower_or_digit(ctx: &mut noprop::TestCaseContext) -> char {
    let pick = noprop::sample_usize_in(ctx, 0..LOWER_ALNUM_CHARSET);
    if pick < LOWER_CHARSET {
        (b'a' + pick as u8) as char
    } else {
        (b'0' + (pick as u8 - LOWER_CHARSET as u8)) as char
    }
}

/// 有効なヘッダー名を生成する (小文字 ASCII)
fn sample_valid_header_name(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-_";
    let len = noprop::sample_usize_in(ctx, 1..=16);
    (0..len)
        .map(|_| noprop::sample_choice(ctx, CHARSET))
        .collect()
}

/// 有効なヘッダー値を生成する (印字可能 ASCII)
///
/// RFC 9113 Section 8.2.1: 先頭/末尾の SP (0x20) / HTAB (0x09) は禁止。
/// 空値または先頭/末尾が SP/HTAB でない値を生成する。
fn sample_valid_header_value(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    let len = noprop::sample_usize_in(ctx, 0..=32);
    let mut v: Vec<u8> = (0..len)
        .map(|_| noprop::sample_u64_in(ctx, 0x20..=0x7E) as u8)
        .collect();
    // 先頭の SP/HTAB を除去
    while v.first().is_some_and(|&b| b == 0x20 || b == 0x09) {
        v.remove(0);
    }
    // 末尾の SP/HTAB を除去
    while v.last().is_some_and(|&b| b == 0x20 || b == 0x09) {
        v.pop();
    }
    v
}

/// HTTP メソッドを生成する
fn sample_http_method(ctx: &mut noprop::TestCaseContext) -> &'static str {
    noprop::sample_choice(
        ctx,
        &["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH"],
    )
}

/// HTTP スキームを生成する
fn sample_http_scheme(ctx: &mut noprop::TestCaseContext) -> &'static str {
    noprop::sample_choice(ctx, &["http", "https"])
}

/// 有効な HTTP パスを生成する
fn sample_http_path(ctx: &mut noprop::TestCaseContext) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789/-_.";
    let len = noprop::sample_usize_in(ctx, 1..=32);
    let mut path = String::from("/");
    for _ in 0..len {
        path.push(noprop::sample_choice(ctx, CHARSET) as char);
    }
    path
}

/// HTTP ステータスコードを生成する
fn sample_http_status(ctx: &mut noprop::TestCaseContext) -> String {
    noprop::sample_choice(
        ctx,
        &[
            "200", "201", "204", "301", "302", "400", "401", "403", "404", "500", "502", "503",
        ],
    )
    .to_string()
}

/// 例: `[a-z][a-z0-9]{0,10}.[a-z]{2,3}` のフォームの authority を生成する
fn sample_authority(ctx: &mut noprop::TestCaseContext) -> String {
    let mut s = String::new();
    s.push(sample_lower_ascii(ctx));
    let middle_len = noprop::sample_usize_in(ctx, 0..=10);
    for _ in 0..middle_len {
        s.push(sample_lower_or_digit(ctx));
    }
    s.push('.');
    let tld_len = 2 + noprop::sample_usize_in(ctx, 0..2);
    for _ in 0..tld_len {
        s.push(sample_lower_ascii(ctx));
    }
    s
}

/// 有効なリクエストヘッダーは検証を通過する
#[test]
fn prop_valid_request_passes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let method = sample_http_method(ctx);
        let scheme = sample_http_scheme(ctx);
        let path = sample_http_path(ctx);
        let authority = sample_authority(ctx);
        let regular_header_count = noprop::sample_usize_in(ctx, 0..=4);
        let regular_headers: Vec<(Vec<u8>, Vec<u8>)> = (0..regular_header_count)
            .map(|_| {
                (
                    sample_valid_header_name(ctx),
                    sample_valid_header_value(ctx),
                )
            })
            .collect();

        let mut headers = vec![
            HeaderField::new(":method", method).expect("valid header field"),
            HeaderField::new(":scheme", scheme).expect("valid header field"),
            HeaderField::new(":path", &path).expect("valid header field"),
            HeaderField::new(":authority", &authority).expect("valid header field"),
        ];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !FORBIDDEN_FOR_VALIDATION.contains(&name_str.as_ref()) {
                headers.push(HeaderField::new(name, value).expect("valid header field"));
            }
        }

        assert!(validation::validate_request_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// 有効なレスポンスヘッダーは検証を通過する
#[test]
fn prop_valid_response_passes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let status = sample_http_status(ctx);
        let regular_header_count = noprop::sample_usize_in(ctx, 0..=4);
        let regular_headers: Vec<(Vec<u8>, Vec<u8>)> = (0..regular_header_count)
            .map(|_| {
                (
                    sample_valid_header_name(ctx),
                    sample_valid_header_value(ctx),
                )
            })
            .collect();

        let mut headers = vec![HeaderField::new(":status", &status).expect("valid header field")];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !FORBIDDEN_FOR_VALIDATION.contains(&name_str.as_ref()) {
                headers.push(HeaderField::new(name, value).expect("valid header field"));
            }
        }

        assert!(validation::validate_response_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// 有効な CONNECT リクエストは検証を通過する
#[test]
fn prop_valid_connect_passes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let host = sample_authority(ctx);
        let port = 1 + noprop::sample_u64_in(ctx, 0..=65534) as u16;
        let authority = format!("{host}:{port}");
        let regular_header_count = noprop::sample_usize_in(ctx, 0..=4);
        let regular_headers: Vec<(Vec<u8>, Vec<u8>)> = (0..regular_header_count)
            .map(|_| {
                (
                    sample_valid_header_name(ctx),
                    sample_valid_header_value(ctx),
                )
            })
            .collect();

        let mut headers = vec![
            HeaderField::new(":method", "CONNECT").expect("valid header field"),
            HeaderField::new(":authority", &authority).expect("valid header field"),
        ];

        for (name, value) in regular_headers {
            // 禁止ヘッダーを避ける
            let name_str = String::from_utf8_lossy(&name);
            if !FORBIDDEN_FOR_VALIDATION.contains(&name_str.as_ref()) {
                headers.push(HeaderField::new(name, value).expect("valid header field"));
            }
        }

        assert!(validation::validate_request_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// 禁止ヘッダーは拒否される
#[test]
fn prop_forbidden_headers_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let forbidden = noprop::sample_choice(
            ctx,
            &[
                "connection",
                "keep-alive",
                "proxy-connection",
                "transfer-encoding",
                "upgrade",
            ],
        );
        let value = sample_valid_header_value(ctx);
        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(forbidden.as_bytes(), value).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// 大文字を含むヘッダー名は拒否される (RFC 9113 Section 8.2.1)
#[test]
fn prop_uppercase_header_name_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let prefix_len = noprop::sample_usize_in(ctx, 1..=8);
        let suffix_len = noprop::sample_usize_in(ctx, 1..=8);
        let mut name: Vec<u8> = (0..prefix_len)
            .map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8)
            .collect();
        name.push(b'X'); // 大文字 X を挿入
        name.extend(
            (0..suffix_len).map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8),
        );

        // HeaderField::new は構築時に弾くため、validation 経路の検査を確認するには
        // wire 上の不正データを模擬する from_validated_parts で構築する。
        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            pbt::wire_header_field(&name, b"value"),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// 有効なトレーラーは検証を通過する
#[test]
fn prop_valid_trailers_passes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let trailer_count = noprop::sample_usize_in(ctx, 0..=4);
        let trailers: Vec<HeaderField> = (0..trailer_count)
            .filter_map(|_| {
                // 禁止ヘッダーを避ける
                let name = sample_valid_header_name(ctx);
                let value = sample_valid_header_value(ctx);
                let name_str = String::from_utf8_lossy(&name);
                if FORBIDDEN_FOR_VALIDATION.contains(&name_str.as_ref()) {
                    None
                } else {
                    Some(HeaderField::new(name, value).expect("valid header field"))
                }
            })
            .collect();

        assert!(validation::validate_trailers(&trailers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// 有効な Extended CONNECT リクエストは検証を通過する (RFC 8441)
///
/// draft-ietf-webtrans-http2-15 Section 3.2: :protocol=webtransport は :scheme=https が必須。
/// protocol を先に決めて scheme を依存生成することで拒否なしで valid-by-construction にする。
#[test]
fn prop_valid_extended_connect_passes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let path = sample_http_path(ctx);
        let authority = {
            let host = sample_authority(ctx);
            let port = noprop::sample_u64_in(ctx, 0..=65535) as u16;
            format!("{host}:{port}")
        };
        let protocol = noprop::sample_choice(ctx, &["webtransport", "websocket"]);
        // webtransport は https が必須 (draft-ietf-webtrans-http2-15 Section 3.2)
        let scheme = if protocol == "webtransport" {
            "https"
        } else {
            noprop::sample_choice(ctx, &["http", "https"])
        };
        let headers = vec![
            HeaderField::new(":method", "CONNECT").expect("valid header field"),
            HeaderField::new(":scheme", scheme).expect("valid header field"),
            HeaderField::new(":path", &path).expect("valid header field"),
            HeaderField::new(":authority", &authority).expect("valid header field"),
            HeaderField::new(":protocol", protocol).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// Host と :authority が不一致の場合は拒否される (RFC 9113 Section 8.3.1)
#[test]
fn prop_host_authority_mismatch_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let authority = sample_authority(ctx);
        // 末尾に '.suffix' を追加するため authority と一致することはない (valid-by-construction)
        let suffix_len = noprop::sample_usize_in(ctx, 1..=5);
        let mut host = authority.clone();
        host.push('.');
        for _ in 0..suffix_len {
            host.push(sample_lower_ascii(ctx));
        }
        let scheme = sample_http_scheme(ctx);
        let path = sample_http_path(ctx);
        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", scheme).expect("valid header field"),
            HeaderField::new(":path", &path).expect("valid header field"),
            HeaderField::new(":authority", &authority).expect("valid header field"),
            HeaderField::new("host", &host).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// Host と :authority が一致する場合は通過する (RFC 9113 Section 8.3.1)
#[test]
fn prop_host_authority_match_accepted() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let authority = sample_authority(ctx);
        let scheme = sample_http_scheme(ctx);
        let path = sample_http_path(ctx);
        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", scheme).expect("valid header field"),
            HeaderField::new(":path", &path).expect("valid header field"),
            HeaderField::new(":authority", &authority).expect("valid header field"),
            HeaderField::new("host", &authority).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// http/https スキームで :authority も Host もない場合は拒否される (RFC 9113 Section 8.3.1)
#[test]
fn prop_http_request_without_authority_or_host_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let method = sample_http_method(ctx);
        let scheme = sample_http_scheme(ctx);
        let path = sample_http_path(ctx);
        let headers = vec![
            HeaderField::new(":method", method).expect("valid header field"),
            HeaderField::new(":scheme", scheme).expect("valid header field"),
            HeaderField::new(":path", &path).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// :authority がなくても Host があれば通過する (RFC 9113 Section 8.3.1)
#[test]
fn prop_http_request_with_host_only_passes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let method = sample_http_method(ctx);
        let scheme = sample_http_scheme(ctx);
        let path = sample_http_path(ctx);
        let host = sample_authority(ctx);
        let headers = vec![
            HeaderField::new(":method", method).expect("valid header field"),
            HeaderField::new(":scheme", scheme).expect("valid header field"),
            HeaderField::new(":path", &path).expect("valid header field"),
            HeaderField::new("host", &host).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// 制御文字、スペース、デリミタ、大文字を含むヘッダー名は拒否される (RFC 9110 Section 5.6.2 / RFC 9113 Section 8.2.1)
#[test]
fn prop_header_name_with_invalid_chars_rejected() -> noprop::TestResult {
    const INVALID_CHARS: &[u8] = &[
        0x00, 0x01, 0x09, 0x20, b'(', b')', b'<', b'>', b'@', b',', b';', b':', b'\\', b'"', b'/',
        b'[', b']', b'?', b'=', b'{', b'}', b'A', b'Z',
    ];
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let prefix_len = noprop::sample_usize_in(ctx, 1..=4);
        let suffix_len = noprop::sample_usize_in(ctx, 1..=4);
        let mut name: Vec<u8> = (0..prefix_len)
            .map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8)
            .collect();
        name.push(noprop::sample_choice(ctx, INVALID_CHARS));
        name.extend(
            (0..suffix_len).map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8),
        );

        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
            // HeaderField::new は構築時に弾くため、wire 由来データを模擬するために
            // from_validated_parts を使って validation 経路の検査を確認する。
            pbt::wire_header_field(&name, b"value"),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// NUL を含むヘッダー値は拒否される (RFC 9110 Section 5.5)
#[test]
fn prop_header_value_with_nul_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let prefix_len = noprop::sample_usize_in(ctx, 0..=4);
        let suffix_len = noprop::sample_usize_in(ctx, 0..=4);
        let mut value: Vec<u8> = (0..prefix_len)
            .map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8)
            .collect();
        value.push(0x00);
        value.extend(
            (0..suffix_len).map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8),
        );

        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
            pbt::wire_header_field(b"x-test", &value),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// CR/LF を含むヘッダー値は拒否される (RFC 9110 Section 5.5)
#[test]
fn prop_header_value_with_cr_lf_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let prefix_len = noprop::sample_usize_in(ctx, 0..=4);
        let suffix_len = noprop::sample_usize_in(ctx, 0..=4);
        let bad_byte = noprop::sample_choice(ctx, &[0x0du8, 0x0a]);
        let mut value: Vec<u8> = (0..prefix_len)
            .map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8)
            .collect();
        value.push(bad_byte);
        value.extend(
            (0..suffix_len).map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8),
        );

        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
            pbt::wire_header_field(b"x-test", &value),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// 先頭に SP/HTAB があるヘッダー値は拒否される (RFC 9113 Section 8.2.1)
#[test]
fn prop_header_value_leading_whitespace_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let ws = noprop::sample_choice(ctx, &[0x20u8, 0x09]);
        let body_len = noprop::sample_usize_in(ctx, 1..=8);
        let mut value = vec![ws];
        value.extend(
            (0..body_len).map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8),
        );

        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
            pbt::wire_header_field(b"x-test", &value),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// 末尾に SP/HTAB があるヘッダー値は拒否される (RFC 9113 Section 8.2.1)
#[test]
fn prop_header_value_trailing_whitespace_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let body_len = noprop::sample_usize_in(ctx, 1..=8);
        let ws = noprop::sample_choice(ctx, &[0x20u8, 0x09]);
        let mut value: Vec<u8> = (0..body_len)
            .map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8)
            .collect();
        value.push(ws);

        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
            pbt::wire_header_field(b"x-test", &value),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// 内部の SP はヘッダー値として許可される (RFC 9113 Section 8.2.1)
#[test]
fn prop_header_value_internal_space_accepted() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let prefix_len = noprop::sample_usize_in(ctx, 1..=8);
        let suffix_len = noprop::sample_usize_in(ctx, 1..=8);
        let mut value: Vec<u8> = (0..prefix_len)
            .map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8)
            .collect();
        value.push(b' ');
        value.extend(
            (0..suffix_len).map(|_| noprop::sample_u64_in(ctx, b'a' as u64..=b'z' as u64) as u8),
        );

        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "https").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", "example.com").expect("valid header field"),
            pbt::wire_header_field(b"x-test", &value),
        ];

        assert!(validation::validate_request_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// CONNECT の :authority が authority-form (host:port) なら通過する (RFC 9113 Section 8.5)
#[test]
fn prop_connect_authority_with_port_accepted() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let host = sample_authority(ctx);
        let port = 1 + noprop::sample_u64_in(ctx, 0..=65534) as u16;
        let authority = format!("{host}:{port}");
        let headers = vec![
            HeaderField::new(":method", "CONNECT").expect("valid header field"),
            HeaderField::new(":authority", &authority).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// http/https スキームで :authority に userinfo (@) がある場合は拒否される (RFC 9113 Section 8.3.1)
#[test]
fn prop_userinfo_in_authority_rejected_for_http() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let user_len = noprop::sample_usize_in(ctx, 1..=4);
        let mut user = String::new();
        for _ in 0..user_len {
            user.push(sample_lower_ascii(ctx));
        }
        let host = sample_authority(ctx);
        let scheme = sample_http_scheme(ctx);
        let authority = format!("{user}@{host}");
        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", scheme).expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", &authority).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_err());
        Ok(())
    })?;
    Ok(())
}

/// 非 HTTP スキームで :authority に userinfo (@) があっても通過する (RFC 9113 Section 8.3.1)
#[test]
fn prop_userinfo_in_authority_accepted_for_non_http() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let user_len = noprop::sample_usize_in(ctx, 1..=4);
        let mut user = String::new();
        for _ in 0..user_len {
            user.push(sample_lower_ascii(ctx));
        }
        let host = sample_authority(ctx);
        let authority = format!("{user}@{host}");
        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", "ftp").expect("valid header field"),
            HeaderField::new(":path", "/").expect("valid header field"),
            HeaderField::new(":authority", &authority).expect("valid header field"),
        ];

        assert!(validation::validate_request_headers(&headers).is_ok());
        Ok(())
    })?;
    Ok(())
}

/// RFC 9113 §8.3.1: http/https (大文字小文字不問) で :path 空は EmptyPath エラーとなり、
/// それ以外のスキームでは :path 空が許容されることを検証する
#[test]
fn prop_empty_path_scheme_dependent() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let scheme = sample_scheme_with_http_and_others(ctx);
        let headers = vec![
            HeaderField::new(":method", "GET").expect("valid header field"),
            HeaderField::new(":scheme", &scheme).expect("valid header field"),
            HeaderField::new(":path", "").expect("valid header field"),
        ];
        let result = validation::validate_request_headers(&headers);
        let is_http_or_https =
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
        if is_http_or_https {
            assert!(
                result.is_err(),
                "http/https スキームで :path 空は拒否されるべき: scheme={scheme}"
            );
        } else {
            assert!(
                result.is_ok(),
                "http/https 以外で :path 空は許容されるべき: scheme={scheme}"
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// http/https (大文字小文字任意) とそれ以外のスキームを混ぜて生成する
fn sample_scheme_with_http_and_others(ctx: &mut noprop::TestCaseContext) -> String {
    match noprop::sample_weighted_index(ctx, &[2, 5]) {
        0 => {
            // http または https の大文字小文字任意な綴り
            let spellings = ["http", "https"];
            let base = noprop::sample_choice(ctx, &spellings[..]);
            base.chars()
                .map(|c| {
                    if noprop::sample_bool(ctx) {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    }
                })
                .collect()
        }
        _ => {
            // http/https 以外 (長さ 1-3, 6-8 で衝突を回避)
            // RFC 9110 Section 3.1 の scheme 文法: 先頭は ALPHA、以降に ALPHA/DIGIT/+/./- 可
            const CHARSET: &[u8] =
                b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-.";
            const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
            let len = noprop::sample_choice(ctx, &[1usize, 2, 3, 6, 7, 8]);
            let mut s = String::new();
            s.push(noprop::sample_choice(ctx, ALPHA) as char);
            for _ in 1..len {
                s.push(noprop::sample_choice(ctx, CHARSET) as char);
            }
            s
        }
    }
}
