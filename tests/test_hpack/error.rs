use shiguredo_http2::HeaderFieldError;

#[test]
fn display_empty_field_name() {
    assert_eq!(
        HeaderFieldError::EmptyFieldName.to_string(),
        "field name must not be empty"
    );
}

#[test]
fn display_uppercase_field_name() {
    let err = HeaderFieldError::UppercaseFieldName {
        name: b"Host".to_vec(),
    };
    assert_eq!(err.to_string(), "field name must be lowercase: Host");
}

#[test]
fn display_invalid_field_name_byte() {
    let err = HeaderFieldError::InvalidFieldNameByte {
        name: b"x foo".to_vec(),
        byte: b' ',
    };
    assert_eq!(
        err.to_string(),
        "field name contains invalid byte 0x20: x foo"
    );
}

#[test]
fn display_invalid_field_value_byte() {
    let err = HeaderFieldError::InvalidFieldValueByte {
        name: b":path".to_vec(),
        byte: 0x0d,
    };
    assert_eq!(
        err.to_string(),
        "field value of :path contains forbidden byte 0x0d"
    );
}

#[test]
fn display_field_value_whitespace() {
    let err = HeaderFieldError::FieldValueLeadingOrTrailingWhitespace {
        name: b"content-type".to_vec(),
    };
    assert_eq!(
        err.to_string(),
        "field value of content-type must not start or end with SP/HTAB"
    );
}

#[test]
fn display_unknown_pseudo_header() {
    let err = HeaderFieldError::UnknownPseudoHeader {
        name: b":foo".to_vec(),
    };
    assert_eq!(err.to_string(), "unknown pseudo-header: :foo");
}

#[test]
fn display_invalid_pseudo_header_value() {
    let err = HeaderFieldError::InvalidPseudoHeaderValue {
        name: b":status".to_vec(),
        value: b"abc".to_vec(),
    };
    assert_eq!(
        err.to_string(),
        "invalid value for pseudo-header :status: abc"
    );
}
