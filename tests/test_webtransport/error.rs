use shiguredo_http2::webtransport::{WtError, WtErrorKind};

/// WtError::Display 出力にファイルパスが含まれないこと
#[test]
fn test_display_excludes_location() {
    let err = WtError::invalid_input("test reason");
    let display = format!("{err}");
    assert!(
        !display.contains('/'),
        "Display にファイルパスが含まれていないこと: {display}"
    );
    assert!(
        !display.contains('\\'),
        "Display にファイルパスが含まれていないこと: {display}"
    );
    assert!(
        display.contains("InvalidInput"),
        "Display にエラー種別が含まれていること: {display}"
    );
    assert!(
        display.contains("test reason"),
        "Display に理由が含まれていること: {display}"
    );
}

/// WtError::Display 出力に "Backtrace" 文字列が含まれず、reason 空時は kind のみになること
#[test]
fn test_display_excludes_backtrace() {
    let err = WtError::new(WtErrorKind::Incomplete);
    let display = format!("{err}");
    assert!(
        !display.contains("Backtrace"),
        "Display に Backtrace が含まれていないこと: {display}"
    );
    assert_eq!(
        display, "Incomplete",
        "reason 空時は kind のみ出力されること"
    );
}

/// WtError::Debug (通常) にバックトレースが含まれず、location は含まれること
#[test]
fn test_debug_excludes_backtrace() {
    let err = WtError::invalid_input("test reason");
    let debug = format!("{err:?}");
    assert!(
        !debug.contains("Backtrace"),
        "Debug 通常に Backtrace が含まれていないこと: {debug}"
    );
    assert!(
        debug.contains("InvalidInput"),
        "Debug 通常にエラー種別が含まれていること: {debug}"
    );
    assert!(
        debug.contains("test reason"),
        "Debug 通常に理由が含まれていること: {debug}"
    );
    assert!(
        debug.contains('/'),
        "Debug 通常に location (ファイルパス) が含まれていること: {debug}"
    );
}

/// WtError::Debug (alternate) でも location が含まれること
/// (Backtrace の検証は RUST_BACKTRACE 環境依存のため行わない)
#[test]
fn test_debug_alternate_accepts_backtrace() {
    let err = WtError::invalid_input("test reason");
    let alt_debug = format!("{err:#?}");
    assert!(
        alt_debug.contains('/'),
        "Debug alternate にファイルパスが含まれていること: {alt_debug}"
    );
}
