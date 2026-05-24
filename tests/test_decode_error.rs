use shiguredo_http2::DecodeError;

#[test]
fn check_buffer_size_ok() {
    let buf = [0u8; 16];
    assert_eq!(DecodeError::check_buffer_size(8, &buf), Ok(()));
    assert_eq!(DecodeError::check_buffer_size(16, &buf), Ok(()));
}

#[test]
fn check_buffer_size_err() {
    let buf = [0u8; 8];
    assert_eq!(
        DecodeError::check_buffer_size(16, &buf),
        Err(DecodeError::BufferTooShort {
            required: 16,
            available: 8,
        })
    );
}

#[test]
fn display_buffer_too_short() {
    let err = DecodeError::BufferTooShort {
        required: 9,
        available: 4,
    };
    assert_eq!(
        err.to_string(),
        "buffer too short: required 9 bytes, available 4 bytes"
    );
}

#[test]
fn display_incomplete() {
    assert_eq!(DecodeError::Incomplete.to_string(), "incomplete input");
}
