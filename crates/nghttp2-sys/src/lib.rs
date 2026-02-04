//! nghttp2 FFI バインディング
//!
//! このクレートは nghttp2 C ライブラリへの低レベル FFI バインディングを提供する。

#![allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    clippy::all
)]

include!("bindings.rs");
