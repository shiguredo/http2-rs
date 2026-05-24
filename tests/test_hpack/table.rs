use shiguredo_http2::hpack::table::{STATIC_TABLE, find_static_index, get_static_entry};
use shiguredo_http2::hpack::{HeaderField, HeaderFieldError};

#[test]
fn static_table_count() {
    assert_eq!(STATIC_TABLE.len(), 62);
}

#[test]
fn get_static_entry_basic() {
    let entry = get_static_entry(1).unwrap();
    assert_eq!(entry.name, b":authority");
    assert_eq!(entry.value, b"");

    let entry = get_static_entry(2).unwrap();
    assert_eq!(entry.name, b":method");
    assert_eq!(entry.value, b"GET");

    let entry = get_static_entry(61).unwrap();
    assert_eq!(entry.name, b"www-authenticate");
    assert_eq!(entry.value, b"");

    assert!(get_static_entry(0).is_none());
    assert!(get_static_entry(62).is_none());
}

#[test]
fn find_static_index_basic() {
    let result = find_static_index(b":method", b"GET");
    assert_eq!(result, Some((2, true)));

    let result = find_static_index(b":method", b"PUT");
    assert_eq!(result, Some((2, false)));

    let result = find_static_index(b"x-custom-header", b"value");
    assert_eq!(result, None);
}

#[test]
fn header_field_size() {
    let field = HeaderField::new("content-type", "application/json").unwrap();
    // 12 + 16 + 32 = 60
    assert_eq!(field.size(), 60);
}

#[test]
fn header_field_new_accepts_valid() {
    let h = HeaderField::new(":method", "GET").unwrap();
    assert_eq!(h.name(), b":method");
    assert_eq!(h.value(), b"GET");
    assert!(!h.sensitive());
}

#[test]
fn header_field_new_with_sensitive() {
    let h = HeaderField::new_with_sensitive("authorization", "Bearer secret", true).unwrap();
    assert!(h.sensitive());
    assert_eq!(h.name(), b"authorization");
    assert_eq!(h.value(), b"Bearer secret");
}

#[test]
fn header_field_new_rejects_empty_name() {
    let err = HeaderField::new("", "value").unwrap_err();
    assert!(matches!(err, HeaderFieldError::EmptyFieldName));
}

#[test]
fn header_field_new_rejects_uppercase_name() {
    let err = HeaderField::new("Content-Type", "text/html").unwrap_err();
    assert!(matches!(err, HeaderFieldError::UppercaseFieldName { .. }));
}

#[test]
fn header_field_new_rejects_invalid_name_byte() {
    let err = HeaderField::new("foo bar", "v").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::InvalidFieldNameByte { byte: b' ', .. }
    ));
}

#[test]
fn header_field_new_rejects_colon_in_middle() {
    let err = HeaderField::new("foo:bar", "v").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::InvalidFieldNameByte { byte: b':', .. }
    ));
}

#[test]
fn header_field_new_rejects_crlf_in_value() {
    let err = HeaderField::new(":path", "/\r\nX-Inject: 1").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::InvalidFieldValueByte { byte: 0x0d, .. }
    ));
}

#[test]
fn header_field_new_rejects_nul_in_value() {
    let err = HeaderField::new("x", "abc\0def").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::InvalidFieldValueByte { byte: 0x00, .. }
    ));
}

#[test]
fn header_field_new_rejects_leading_whitespace() {
    let err = HeaderField::new("x", " value").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::FieldValueLeadingOrTrailingWhitespace { .. }
    ));
}

#[test]
fn header_field_new_rejects_trailing_tab() {
    let err = HeaderField::new("x", "value\t").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::FieldValueLeadingOrTrailingWhitespace { .. }
    ));
}

#[test]
fn header_field_new_rejects_unknown_pseudo() {
    let err = HeaderField::new(":foo", "bar").unwrap_err();
    assert!(matches!(err, HeaderFieldError::UnknownPseudoHeader { .. }));
}

#[test]
fn header_field_new_rejects_invalid_status() {
    let err = HeaderField::new(":status", "abc").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::InvalidPseudoHeaderValue { .. }
    ));
}

#[test]
fn header_field_new_accepts_status_200() {
    let h = HeaderField::new(":status", "200").unwrap();
    assert_eq!(h.value(), b"200");
}

#[test]
fn header_field_new_rejects_invalid_method() {
    let err = HeaderField::new(":method", "GE T").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::InvalidPseudoHeaderValue { .. }
    ));
}

#[test]
fn header_field_new_accepts_scheme_https() {
    let h = HeaderField::new(":scheme", "https").unwrap();
    assert_eq!(h.value(), b"https");
}

#[test]
fn header_field_new_rejects_invalid_scheme() {
    let err = HeaderField::new(":scheme", "1http").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::InvalidPseudoHeaderValue { .. }
    ));
}

#[test]
fn header_field_new_accepts_path_absolute() {
    let h = HeaderField::new(":path", "/index.html").unwrap();
    assert_eq!(h.value(), b"/index.html");
}

#[test]
fn header_field_new_accepts_path_asterisk() {
    let h = HeaderField::new(":path", "*").unwrap();
    assert_eq!(h.value(), b"*");
}

#[test]
fn header_field_new_accepts_path_empty() {
    // 空 :path は scheme 依存のため構築時には弾かない (validation.rs 側で判定)
    let h = HeaderField::new(":path", "").unwrap();
    assert_eq!(h.value(), b"");
}

#[test]
fn header_field_new_rejects_path_non_absolute() {
    let err = HeaderField::new(":path", "index.html").unwrap_err();
    assert!(matches!(
        err,
        HeaderFieldError::InvalidPseudoHeaderValue { .. }
    ));
}

#[test]
fn header_field_from_static_pseudo() {
    const M: HeaderField = HeaderField::from_static(b":method", b"GET");
    assert_eq!(M.name(), b":method");
    assert_eq!(M.value(), b"GET");
    assert!(!M.sensitive());
}

#[test]
fn header_field_from_static_regular() {
    const H: HeaderField = HeaderField::from_static(b"content-type", b"text/html");
    assert_eq!(H.name(), b"content-type");
    assert_eq!(H.value(), b"text/html");
}

#[test]
fn header_field_cross_variant_eq() {
    // from_static (Cow::Borrowed) と new (Cow::Owned) の PartialEq 一致を検証する
    const STATIC: HeaderField = HeaderField::from_static(b":method", b"GET");
    let runtime = HeaderField::new(":method", "GET").expect("valid header field");
    assert_eq!(STATIC, runtime);
    assert_eq!(runtime, STATIC);
}

#[test]
fn header_field_cross_variant_hash() {
    // from_static (Cow::Borrowed) と new (Cow::Owned) の Hash 一致を検証する
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    const STATIC: HeaderField = HeaderField::from_static(b"content-type", b"text/html");
    let runtime = HeaderField::new("content-type", "text/html").expect("valid header field");
    let mut hs = DefaultHasher::new();
    STATIC.hash(&mut hs);
    let mut hr = DefaultHasher::new();
    runtime.hash(&mut hr);
    assert_eq!(hs.finish(), hr.finish());
}

#[test]
fn header_field_cross_variant_size() {
    // from_static (Cow::Borrowed) と new (Cow::Owned) の size() 一致を検証する
    const STATIC: HeaderField = HeaderField::from_static(b":status", b"200");
    let runtime = HeaderField::new(":status", "200").expect("valid header field");
    assert_eq!(STATIC.size(), runtime.size());
}
