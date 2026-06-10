use shiguredo_http2::{FlowControl, MAX_WINDOW_SIZE};

#[test]
fn test_new_flow_control() {
    let fc = FlowControl::new(65535);
    assert_eq!(fc.send_window(), 65535);
    assert_eq!(fc.recv_window(), 65535);
}

#[test]
fn test_consume_send() {
    let mut fc = FlowControl::new(65535);
    fc.consume_send(1000).unwrap();
    assert_eq!(fc.send_window(), 64535);
}

#[test]
fn test_consume_send_exhausted() {
    let mut fc = FlowControl::new(100);
    assert!(fc.consume_send(101).is_err());
}

#[test]
fn test_recv_window_update() {
    let mut fc = FlowControl::new(65535);
    fc.consume_send(10000).unwrap();
    assert_eq!(fc.send_window(), 55535);

    fc.recv_window_update(5000).unwrap();
    assert_eq!(fc.send_window(), 60535);
}

/// RFC 9113 Section 6.9.1: フロー制御ウィンドウは 2^31-1 オクテットを超えてはならない (MUST NOT)。超過させる WINDOW_UPDATE はエラー。
#[test]
fn test_window_update_overflow() {
    let mut fc = FlowControl::new(MAX_WINDOW_SIZE);
    assert!(fc.recv_window_update(1).is_err());
}

#[test]
fn test_should_send_window_update() {
    let mut fc = FlowControl::new(65535);
    assert!(!fc.should_send_window_update());

    fc.consume_recv(40000).unwrap();
    assert!(fc.should_send_window_update());
}

/// RFC 9113 Section 6.9.2: SETTINGS_INITIAL_WINDOW_SIZE 変更時は新旧値の差分で全ストリームのウィンドウを調整しなければならない (MUST)。
#[test]
fn test_update_initial_window_size() {
    let mut fc = FlowControl::new(65535);
    fc.consume_send(10000).unwrap();
    assert_eq!(fc.send_window(), 55535);

    // 初期ウィンドウサイズを増やす
    fc.update_initial_window_size(100000).unwrap();
    // 55535 + (100000 - 65535) = 90000
    assert_eq!(fc.send_window(), 90000);
}

/// RFC 9113 Section 6.9: add_recv_window に increment == 0 を渡すとエラーになる
/// (PROTOCOL_ERROR 相当の境界値)。
#[test]
fn test_add_recv_window_zero_rejected() {
    let mut fc = FlowControl::new(65535);
    assert!(fc.add_recv_window(0).is_err());
}
