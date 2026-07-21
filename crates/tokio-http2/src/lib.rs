//! tokio ベースの HTTP/2 I/O 実装
//!
//! shiguredo_http2 (Sans I/O) を tokio と統合し、非同期 HTTP/2 クライアント/サーバーを提供する。

mod client;
mod connection;
mod error;
mod server;
mod tls;
pub mod webtransport;

pub use client::Client;
pub use connection::Connection;
pub use error::{Error, Result};
pub use server::{Server, ServerConnection};
pub use shiguredo_http2::webtransport::WtError;
pub use shiguredo_http2::{ErrorCode, Event, HeaderField, Limits, LimitsBuilder, StreamId};
pub use tls::{TlsClientConfig, TlsServerConfig};
pub use webtransport::{
    WEBTRANSPORT_PROTOCOL, WtBidiStream, WtServerRequest, WtServerSession, WtSessionHandle,
    WtSessionParts, WtUniRecvStream, WtUniSendStream,
};

/// HTTP/2 コネクションプリフェイス
pub const CONNECTION_PREFACE: &[u8] = shiguredo_http2::CONNECTION_PREFACE;
