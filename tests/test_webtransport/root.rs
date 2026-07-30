use shiguredo_http2::connection::Role;
use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, WtConfig, WtSession, WtSessionState, stream,
};

#[test]
fn test_client_session_creation() {
    let session = WtSession::client(WtConfig::default(), WtConfig::default());
    assert_eq!(session.role(), Role::Client);
    assert_eq!(session.state(), WtSessionState::Initial);
}

#[test]
fn test_server_session_creation() {
    let session = WtSession::server(WtConfig::default(), WtConfig::default());
    assert_eq!(session.role(), Role::Server);
    assert_eq!(session.state(), WtSessionState::Initial);
}

#[test]
fn test_session_initiate() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");
    assert_eq!(session.state(), WtSessionState::Active);
}

#[test]
fn test_open_bidi_stream() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("initiate should succeed");
    assert!(stream::stream_id::is_client_initiated(stream_id));
    assert!(stream::stream_id::is_bidirectional(stream_id));
}

#[test]
fn test_open_uni_stream() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_uni_stream().expect("initiate should succeed");
    assert!(stream::stream_id::is_client_initiated(stream_id));
    assert!(stream::stream_id::is_unidirectional(stream_id));
}

#[test]
fn test_send_datagram() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    session
        .send_datagram(b"hello")
        .expect("initiate should succeed");
    assert!(session.has_output());
}

#[test]
fn test_close_session() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    session
        .close(0, "normal close")
        .expect("initiate should succeed");
    assert_eq!(session.state(), WtSessionState::Closed);
}

#[test]
fn test_drain_session() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    session.drain().expect("initiate should succeed");
    assert_eq!(session.state(), WtSessionState::Draining);
}

/// `close()` の二重呼び出しが `SessionStateError` になることを確認する
#[test]
fn test_close_double_call_errors() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    session
        .close(0, "first close")
        .expect("initiate should succeed");
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
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    session
        .close(42, "reason")
        .expect("initiate should succeed");
    assert!(session.has_output());

    let out = session.poll_output().expect("output expected");

    let mut decoder = CapsuleDecoder::new();
    decoder.feed(&out);
    let capsule = decoder
        .decode()
        .expect("feed should succeed")
        .expect("capsule expected");

    match capsule {
        Capsule::WtCloseSession { error_code, reason } => {
            assert_eq!(error_code, 42);
            assert_eq!(reason, "reason");
        }
        other => panic!("expected WtCloseSession, got {other:?}"),
    }
}

/// reason が 1024 バイトの境界値までは正常に close() できることを確認する。
/// (draft-ietf-webtrans-http2-15 Section 6.12: メッセージ長は 1024 バイトを超えてはならない (MUST NOT))
#[test]
fn test_close_reason_max_length_ok() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let reason = "a".repeat(1024);
    session.close(0, &reason).expect("should succeed");
    assert_eq!(session.state(), WtSessionState::Closed);
}

/// reason が 1024 バイトを超えると UTF-8 境界で切り詰められることを確認する。
/// (draft-ietf-webtrans-http2-15 Section 6.12: 切り詰めは義務ではないが許容される)
#[test]
fn test_close_reason_exceeds_max_length_truncated() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // 1025 バイトの ASCII reason → 1024 バイトに切り詰め
    let reason = "a".repeat(1025);
    session
        .close(0, &reason)
        .expect("truncation should succeed, not error");
    assert_eq!(session.state(), WtSessionState::Closed);

    // 出力された capsule の reason が 1024 バイト以下であることを確認
    let out = session.poll_output().expect("output expected");
    let mut decoder = shiguredo_http2::webtransport::CapsuleDecoder::new();
    decoder.feed(&out);
    let capsule = decoder
        .decode()
        .expect("decode should succeed")
        .expect("capsule expected");
    if let shiguredo_http2::webtransport::Capsule::WtCloseSession { reason, .. } = capsule {
        assert!(
            reason.len() <= 1024,
            "truncated reason should be <= 1024 bytes, got {}",
            reason.len()
        );
    } else {
        panic!("expected WtCloseSession capsule");
    }
}

/// マルチバイト文字が 1024 バイト境界にまたがる場合の切り詰めを確認する。
#[test]
fn test_close_reason_truncation_utf8_boundary() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // 1023 バイトの ASCII + 2 バイト文字 (U+00E9 = é = 0xC3 0xA9) = 1025 バイト
    // 切り詰め後: 1023 バイト (é の 1 バイト目は 1024 バイト目に来るが、
    // continuation byte ではないので 1024 バイト目で切ると不完全な文字になる)
    // 実際には 1023 バイトの ASCII の後に é があり、1024 バイト位置は
    // é の 1 バイト目 (0xC3) なので、後退して 1023 バイトで切る
    let mut reason = "a".repeat(1023);
    reason.push('é'); // 2 バイト文字
    assert_eq!(reason.len(), 1025);

    session
        .close(0, &reason)
        .expect("truncation should succeed");
    assert_eq!(session.state(), WtSessionState::Closed);

    let out = session.poll_output().expect("output expected");
    let mut decoder = shiguredo_http2::webtransport::CapsuleDecoder::new();
    decoder.feed(&out);
    let capsule = decoder
        .decode()
        .expect("decode should succeed")
        .expect("capsule expected");
    if let shiguredo_http2::webtransport::Capsule::WtCloseSession { reason, .. } = capsule {
        // 1023 バイト (é は切り捨て)
        assert_eq!(reason.len(), 1023);
        assert!(reason.chars().all(|c| c == 'a'));
    } else {
        panic!("expected WtCloseSession capsule");
    }
}

/// draft-ietf-webtrans-http2-15 Section 6.2 / 6.3: `reset_stream` / `stop_sending` の
/// 重複送信はエラーになる。
#[test]
fn test_duplicate_operations_are_errors() {
    let error_code = 42;
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("initiate should succeed");

    // reset_stream: 1 回目は成功、2 回目はエラー
    session
        .reset_stream(stream_id, error_code)
        .expect("should succeed");
    assert!(session.reset_stream(stream_id, error_code).is_err());

    // 別のストリームで stop_sending: 1 回目は成功、2 回目はエラー
    let stream_id2 = session
        .open_bidi_stream()
        .expect("open stream should succeed");
    session
        .stop_sending(stream_id2, error_code)
        .expect("open stream should succeed");
    assert!(session.stop_sending(stream_id2, error_code).is_err());
}
