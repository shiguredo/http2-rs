//! tokio ベースの HTTP/2 I/O 実装
//!
//! shiguredo_nghttp2 (Sans I/O) を tokio と統合し、非同期 HTTP/2 クライアント/サーバーを提供する。

mod client;
mod connection;
mod error;
mod server;
mod tls;

pub use client::Client;
pub use connection::Connection;
pub use error::{Error, Result};
pub use server::{Server, ServerConnection};
pub use shiguredo_nghttp2::{
    ErrorCode, FrameType, Header, Http2Event, SessionOptions, SettingsId, StreamId,
};
pub use tls::{TlsClientConfig, TlsServerConfig};

/// HTTP/2 コネクションプリフェイス
pub const CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
