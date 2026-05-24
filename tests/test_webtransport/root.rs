use shiguredo_http2::connection::Role;
use shiguredo_http2::webtransport::{WtConfig, WtSession, WtSessionState, stream};

#[test]
fn test_client_session_creation() {
    let session = WtSession::client(WtConfig::default());
    assert_eq!(session.role(), Role::Client);
    assert_eq!(session.state(), WtSessionState::Initial);
}

#[test]
fn test_server_session_creation() {
    let session = WtSession::server(WtConfig::default());
    assert_eq!(session.role(), Role::Server);
    assert_eq!(session.state(), WtSessionState::Initial);
}

#[test]
fn test_session_initiate() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();
    assert_eq!(session.state(), WtSessionState::Active);
}

#[test]
fn test_open_bidi_stream() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let stream_id = session.open_bidi_stream().unwrap();
    assert!(stream::stream_id::is_client_initiated(stream_id));
    assert!(stream::stream_id::is_bidirectional(stream_id));
}

#[test]
fn test_open_uni_stream() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let stream_id = session.open_uni_stream().unwrap();
    assert!(stream::stream_id::is_client_initiated(stream_id));
    assert!(stream::stream_id::is_unidirectional(stream_id));
}

#[test]
fn test_send_datagram() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    session.send_datagram(b"hello").unwrap();
    assert!(session.has_output());
}

#[test]
fn test_close_session() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    session.close(0, "normal close").unwrap();
    assert_eq!(session.state(), WtSessionState::Closed);
}

#[test]
fn test_drain_session() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    session.drain().unwrap();
    assert_eq!(session.state(), WtSessionState::Draining);
}
