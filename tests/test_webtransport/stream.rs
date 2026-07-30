use shiguredo_http2::webtransport::stream::{RecvState, SendState, WtStream, stream_id};

// draft-ietf-webtrans-http2-15 Section 5.2: クライアント起点は偶数、サーバー起点は奇数、下位から 2 ビット目が単方向/双方向を示す (RFC 9000 Section 2.1 と同セマンティクス)。
#[test]
fn test_stream_id_client_bidi() {
    let id = stream_id::first(true, true);
    assert_eq!(id, 0);
    assert!(stream_id::is_client_initiated(id));
    assert!(stream_id::is_bidirectional(id));

    let next = stream_id::next(id);
    assert_eq!(next, 4);
    assert!(stream_id::is_client_initiated(next));
    assert!(stream_id::is_bidirectional(next));
}

#[test]
fn test_stream_id_server_bidi() {
    let id = stream_id::first(false, true);
    assert_eq!(id, 1);
    assert!(stream_id::is_server_initiated(id));
    assert!(stream_id::is_bidirectional(id));
}

#[test]
fn test_stream_id_client_uni() {
    let id = stream_id::first(true, false);
    assert_eq!(id, 2);
    assert!(stream_id::is_client_initiated(id));
    assert!(stream_id::is_unidirectional(id));
}

#[test]
fn test_stream_id_server_uni() {
    let id = stream_id::first(false, false);
    assert_eq!(id, 3);
    assert!(stream_id::is_server_initiated(id));
    assert!(stream_id::is_unidirectional(id));
}

#[test]
fn test_stream_creation() {
    let stream = WtStream::new(0, 65536, 65536, true, true);
    assert_eq!(stream.id(), 0);
    assert!(stream.is_bidirectional());
    assert_eq!(stream.send_state(), SendState::Ready);
    assert_eq!(stream.recv_state(), RecvState::Recv);
    assert!(stream.can_send());
    assert!(stream.can_recv());
}

#[test]
fn test_send_data() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    stream
        .send_data(100, false)
        .expect("operation should succeed");
    assert_eq!(stream.send_state(), SendState::Send);
    assert_eq!(stream.send_offset(), 100);
    assert!(stream.can_send());

    // draft-ietf-webtrans-http2-15 Section 5.2: FIN 送信で即座に DataRecvd へ遷移
    stream
        .send_data(100, true)
        .expect("operation should succeed");
    assert_eq!(stream.send_state(), SendState::DataRecvd);
    assert_eq!(stream.send_offset(), 200);
    assert!(!stream.can_send());
}

#[test]
fn test_recv_data() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    stream
        .recv_data(100, false)
        .expect("operation should succeed");
    assert_eq!(stream.recv_state(), RecvState::Recv);
    assert_eq!(stream.recv_offset(), 100);
    assert!(stream.can_recv());

    // draft-ietf-webtrans-http2-15 Section 5.2: FIN 受信で即座に DataRecvd へ遷移
    stream
        .recv_data(100, true)
        .expect("operation should succeed");
    assert_eq!(stream.recv_state(), RecvState::DataRecvd);
    assert_eq!(stream.recv_offset(), 200);
}

#[test]
fn test_send_reset() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    stream
        .send_data(100, false)
        .expect("operation should succeed");
    // draft-ietf-webtrans-http2-15 Section 5.2: リセット送信で即座に ResetRecvd へ遷移
    stream.send_reset();
    assert_eq!(stream.send_state(), SendState::ResetRecvd);
    assert!(!stream.can_send());
}

#[test]
fn test_recv_reset() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    stream
        .recv_data(100, false)
        .expect("operation should succeed");
    // draft-ietf-webtrans-http2-15 Section 5.2: リセット受信で即座に ResetRead へ遷移
    stream.recv_reset();
    assert_eq!(stream.recv_state(), RecvState::ResetRead);
    assert!(!stream.can_recv());
}

#[test]
fn test_update_send_max() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);
    assert_eq!(stream.send_available(), 65536);

    stream
        .update_send_max(131072)
        .expect("operation should succeed");
    assert_eq!(stream.send_available(), 131072);

    // draft-ietf-webtrans-http2-15 Section 6.6: 減少はエラー
    assert!(stream.update_send_max(32768).is_err());
}
