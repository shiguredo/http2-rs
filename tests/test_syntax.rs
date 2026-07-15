//! `HeaderField::from_static` による const 構文検査の受け入れテスト

use shiguredo_http2::HeaderField;

#[test]
fn const_check_accepts_valid_pseudo() {
    const _: HeaderField = HeaderField::from_static(b":method", b"GET");
    const _: HeaderField = HeaderField::from_static(b":status", b"200");
    const _: HeaderField = HeaderField::from_static(b":scheme", b"https");
    const _: HeaderField = HeaderField::from_static(b":path", b"/");
    const _: HeaderField = HeaderField::from_static(b":path", b"*");
    const _: HeaderField = HeaderField::from_static(b":protocol", b"webtransport");
}

#[test]
fn const_check_accepts_valid_regular() {
    const _: HeaderField = HeaderField::from_static(b"content-type", b"text/html");
}
