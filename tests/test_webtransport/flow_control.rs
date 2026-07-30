use shiguredo_http2::webtransport::WtFlowControl;

#[test]
fn test_new_flow_control() {
    let fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);
    assert_eq!(fc.send_available(), 65536);
    assert_eq!(fc.recv_available(), 65536);
}

#[test]
fn test_consume_send() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    fc.consume_send(1000).expect("construction should succeed");
    assert_eq!(fc.send_available(), 64536);
    assert_eq!(fc.send_offset(), 1000);
}

#[test]
fn test_consume_send_exhausted() {
    let mut fc = WtFlowControl::new(100, 100, 100, 100, 50, 50);

    fc.consume_send(100).expect("construction should succeed");
    assert!(fc.consume_send(1).is_err());
}

#[test]
fn test_consume_recv() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    fc.consume_recv(1000).expect("construction should succeed");
    assert_eq!(fc.recv_available(), 64536);
    assert_eq!(fc.recv_offset(), 1000);
}

#[test]
fn test_consume_recv_exceeded() {
    let mut fc = WtFlowControl::new(100, 100, 100, 100, 50, 50);

    fc.consume_recv(100).expect("construction should succeed");
    assert!(fc.consume_recv(1).is_err());
}

#[test]
fn test_update_send_max() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    fc.consume_send(65536).expect("construction should succeed");
    assert!(fc.is_send_blocked());

    fc.update_send_max(131072).expect("should succeed");
    assert!(!fc.is_send_blocked());
    assert_eq!(fc.send_available(), 65536);
}

#[test]
fn test_update_send_max_decrease_error() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    // draft-ietf-webtrans-http2-15 Section 6.5: 減少はエラー
    assert!(fc.update_send_max(32768).is_err());
}

#[test]
fn test_stream_limits() {
    let mut fc = WtFlowControl::new(65536, 65536, 2, 2, 1, 1);

    assert!(fc.can_open_bidi_stream());
    assert!(fc.can_open_uni_stream());

    fc.opened_stream(true);
    fc.opened_stream(true);
    assert!(!fc.can_open_bidi_stream());
    assert!(fc.is_bidi_streams_blocked());

    fc.opened_stream(false);
    assert!(!fc.can_open_uni_stream());
    assert!(fc.is_uni_streams_blocked());
}

#[test]
fn test_update_max_streams() {
    let mut fc = WtFlowControl::new(65536, 65536, 2, 2, 1, 1);

    fc.opened_stream(true);
    fc.opened_stream(true);
    assert!(!fc.can_open_bidi_stream());

    fc.update_max_streams(4, true).expect("should succeed");
    assert!(fc.can_open_bidi_stream());
}

#[test]
fn test_update_max_streams_decrease_error() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    // draft-ietf-webtrans-http2-15 Section 6.7: 減少はエラー
    assert!(fc.update_max_streams(50, true).is_err());
    assert!(fc.update_max_streams(25, false).is_err());
}

#[test]
fn test_should_send_max_data() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    assert!(!fc.should_send_max_data(65536));

    fc.consume_recv(40000).expect("should succeed");
    assert!(fc.should_send_max_data(65536));
}

/// add_recv_max が varint MAX_VALUE を超えた場合にエラーを返すことを確認する
#[test]
fn test_add_recv_max_varint_overflow() {
    use shiguredo_http2::webtransport::MAX_VALUE;

    // MAX_VALUE ちょうどまで増加 → 成功
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);
    fc.add_recv_max(MAX_VALUE - 65536).expect("should succeed");
    assert_eq!(fc.recv_max(), MAX_VALUE);

    // これ以上増加 → エラー
    assert!(fc.add_recv_max(1).is_err());
}

/// add_recv_max が saturating_add で u64::MAX に到達した場合もエラーを返すことを確認する
#[test]
fn test_add_recv_max_u64_max_overflow() {
    let mut fc = WtFlowControl::new(u64::MAX, u64::MAX, 100, 100, 50, 50);
    // u64::MAX + 1 は saturating_add で u64::MAX のまま → varint MAX_VALUE 超過でエラー
    assert!(fc.add_recv_max(1).is_err());
}

/// update_max_streams で 2^60 を超える値はエラーになることを確認する
/// (draft-ietf-webtrans-http2-15 Section 6.7)
#[test]
fn test_update_max_streams_exceeds_2_60() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    // 2^60 ちょうどは成功
    fc.update_max_streams(1u64 << 60, true)
        .expect("2^60 は成功すること");

    // 2^60 + 1 はエラー
    assert!(fc.update_max_streams((1u64 << 60) + 1, true).is_err());
    assert!(fc.update_max_streams((1u64 << 60) + 1, false).is_err());
}
