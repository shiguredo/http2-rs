//! ストリーム状態機械の遷移テスト (RFC 9113 Section 5.1)

use shiguredo_http2::stream::{StateMachine, StreamState};

#[test]
fn test_idle_to_open() {
    let mut sm = StateMachine::new();
    assert_eq!(sm.state(), StreamState::Idle);

    sm.send_headers(false)
        .expect("idle から HEADERS 送信は成功する");
    assert_eq!(sm.state(), StreamState::Open);
}

#[test]
fn test_idle_to_half_closed_local() {
    let mut sm = StateMachine::new();
    sm.send_headers(true)
        .expect("idle から end_stream=true の HEADERS 送信は成功する");
    assert_eq!(sm.state(), StreamState::HalfClosedLocal);
}

#[test]
fn test_open_to_half_closed_local() {
    let mut sm = StateMachine::new();
    sm.send_headers(false)
        .expect("idle から HEADERS 送信は成功する");
    sm.send_data(true)
        .expect("open から DATA 送信検証は成功する");
    // send_data は validate のみで状態遷移しない (フロー制御で詰まる可能性のため)
    assert_eq!(sm.state(), StreamState::Open);
    sm.complete_send_data(true)
        .expect("送信完了後の遷移は成功する");
    assert_eq!(sm.state(), StreamState::HalfClosedLocal);
}

#[test]
fn test_open_to_half_closed_remote() {
    let mut sm = StateMachine::new();
    sm.recv_headers(false)
        .expect("idle から HEADERS 受信は成功する");
    sm.recv_data(true)
        .expect("open から end_stream=true の DATA 受信は成功する");
    assert_eq!(sm.state(), StreamState::HalfClosedRemote);
}

#[test]
fn test_half_closed_to_closed() {
    let mut sm = StateMachine::new();
    sm.send_headers(false)
        .expect("idle から HEADERS 送信は成功する");
    sm.send_data(true)
        .expect("open から DATA 送信検証は成功する");
    sm.complete_send_data(true)
        .expect("送信完了後の遷移は成功する");
    assert_eq!(sm.state(), StreamState::HalfClosedLocal);

    sm.recv_data(true)
        .expect("half-closed local から DATA 受信は成功する");
    assert_eq!(sm.state(), StreamState::Closed);
}

#[test]
fn test_rst_stream_closes() {
    let mut sm = StateMachine::new();
    sm.send_headers(false)
        .expect("idle から HEADERS 送信は成功する");
    sm.send_rst_stream();
    assert_eq!(sm.state(), StreamState::Closed);
}

#[test]
fn test_server_response_from_half_closed_remote() {
    // クライアントが end_stream=true でリクエストを送信
    let mut sm = StateMachine::new();
    sm.recv_headers(true)
        .expect("idle から end_stream=true の HEADERS 受信は成功する");
    assert_eq!(sm.state(), StreamState::HalfClosedRemote);

    // サーバーがレスポンスを送信 (end_stream=true)
    sm.send_headers(true)
        .expect("half-closed remote からレスポンス送信は成功する");
    assert_eq!(sm.state(), StreamState::Closed);
}

#[test]
fn test_server_response_with_body_from_half_closed_remote() {
    // クライアントが end_stream=true でリクエストを送信
    let mut sm = StateMachine::new();
    sm.recv_headers(true)
        .expect("idle から end_stream=true の HEADERS 受信は成功する");
    assert_eq!(sm.state(), StreamState::HalfClosedRemote);

    // サーバーがレスポンスヘッダーを送信 (end_stream=false)
    sm.send_headers(false)
        .expect("half-closed remote からレスポンスヘッダー送信は成功する");
    assert_eq!(sm.state(), StreamState::HalfClosedRemote);

    // サーバーがボディを送信 (end_stream=true)
    sm.send_data(true)
        .expect("half-closed remote から DATA 送信検証は成功する");
    // validate のみで遷移しない
    assert_eq!(sm.state(), StreamState::HalfClosedRemote);
    // 実際に送信完了したら Closed
    sm.complete_send_data(true)
        .expect("送信完了後の遷移は成功する");
    assert_eq!(sm.state(), StreamState::Closed);
}

#[test]
fn test_trailer_headers_from_open() {
    let mut sm = StateMachine::new();
    sm.send_headers(false)
        .expect("idle から HEADERS 送信は成功する");
    assert_eq!(sm.state(), StreamState::Open);

    // トレーラーヘッダーを送信
    sm.send_headers(true)
        .expect("open から end_stream=true の HEADERS 送信は成功する");
    assert_eq!(sm.state(), StreamState::HalfClosedLocal);
}
