use shiguredo_http2::connection::Role;
use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, WtConfig, WtSession, WtSessionState, stream,
};

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

/// `close()` の二重呼び出しが `SessionStateError` になることを確認する
#[test]
fn test_close_double_call_errors() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    session.close(0, "first close").unwrap();
    assert_eq!(session.state(), WtSessionState::Closed);

    let err = session.close(1, "second close").unwrap_err();
    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::SessionStateError
    );
}

/// `close()` が WT_CLOSE_SESSION capsule を出力することを確認する
#[test]
fn test_close_emits_wt_close_session_capsule() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    session.close(42, "reason").unwrap();
    assert!(session.has_output());

    let out = session.poll_output().expect("output expected");

    let mut decoder = CapsuleDecoder::new();
    decoder.feed(&out);
    let capsule = decoder.decode().unwrap().expect("capsule expected");

    match capsule {
        Capsule::WtCloseSession { error_code, reason } => {
            assert_eq!(error_code, 42);
            assert_eq!(reason, "reason");
        }
        other => panic!("expected WtCloseSession, got {other:?}"),
    }
}

/// reason が 1024 バイトの境界値までは正常に close() できることを確認する。
#[test]
fn test_close_reason_max_length_ok() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let reason = "a".repeat(1024);
    session.close(0, &reason).unwrap();
    assert_eq!(session.state(), WtSessionState::Closed);
}

/// reason が 1024 バイトを超えると close() がエラーを返すことを確認する。
#[test]
fn test_close_reason_exceeds_max_length_errors() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let reason = "a".repeat(1025);
    let err = session.close(0, &reason).unwrap_err();
    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::CapsuleDecode
    );
}
