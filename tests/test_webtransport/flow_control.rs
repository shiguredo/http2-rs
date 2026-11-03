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
fn test_update_send_max_equal_and_decrease_error() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    // draft-ietf-webtrans-http2-16 Section 6.5: 同値と減少はエラー
    assert!(fc.update_send_max(65536).is_err(), "同値は拒否されるはず");
    assert!(fc.update_send_max(32768).is_err(), "減少は拒否されるはず");
    assert_eq!(
        fc.send_available(),
        65536,
        "拒否時に送信上限が変わってはいけない"
    );

    // 初期値 0 に対する最初の値 0 も拒否する
    let mut fc = WtFlowControl::new(0, 0, 0, 0, 0, 0);
    assert!(
        fc.update_send_max(0).is_err(),
        "初期値 0 と同値の最初の値も拒否されるはず"
    );
}

/// update_send_max が varint 上限を超える値を拒否することを確認する
/// (RFC 9000 Section 16)
#[test]
fn test_update_send_max_varint_overflow() {
    use shiguredo_http2::webtransport::MAX_VALUE;

    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    fc.update_send_max(MAX_VALUE)
        .expect("varint 上限ちょうどは受理されるはず");
    assert_eq!(fc.send_available(), MAX_VALUE);

    // varint 上限を超える値は拒否され、送信上限は変わらない
    assert!(fc.update_send_max(MAX_VALUE + 1).is_err());
    assert_eq!(fc.send_available(), MAX_VALUE);
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

    fc.update_max_streams(4, true)
        .expect("増加する上限は受理されなければならない");
    assert!(fc.can_open_bidi_stream());
}

#[test]
fn test_update_max_streams_non_increase_error() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    // draft-ietf-webtrans-http2-16 Section 6.7: 同値・減少はエラー
    assert!(
        fc.update_max_streams(100, true).is_err(),
        "bidi の同値は拒否されなければならない"
    );
    assert!(
        fc.update_max_streams(50, false).is_err(),
        "uni の同値は拒否されなければならない"
    );
    assert!(fc.update_max_streams(50, true).is_err());
    assert!(fc.update_max_streams(25, false).is_err());
}

/// 一方の方向の更新と拒否が他方の上限を変更しないことを確認する
///
/// draft-ietf-webtrans-http2-16 Section 6.7。
#[test]
fn test_update_max_streams_direction_independence() {
    let mut fc = WtFlowControl::new(65536, 65536, 2, 2, 1, 1);

    // 両方向とも上限まで開設して can_open_* を偽にしておく
    fc.opened_stream(true);
    fc.opened_stream(true);
    fc.opened_stream(false);
    assert!(!fc.can_open_bidi_stream());
    assert!(!fc.can_open_uni_stream());

    // bidi の更新は bidi のピア上限だけを進め、uni の開設可否は変わらない
    fc.update_max_streams(3, true)
        .expect("bidi の増加は受理されなければならない");
    assert!(fc.can_open_bidi_stream(), "bidi の開設可否が変わる");
    assert!(!fc.can_open_uni_stream(), "uni の開設可否は変わらない");
    assert_eq!(fc.max_streams_bidi_local(), 2, "ローカル上限は不変");
    assert_eq!(fc.max_streams_bidi_remote(), 3, "ピア上限が進む");
    assert_eq!(fc.max_streams_uni_local(), 1, "uni のローカル上限は不変");
    assert_eq!(fc.max_streams_uni_remote(), 1, "uni のピア上限は不変");

    // uni の更新は uni のピア上限だけを進め、bidi の開設可否は変わらない
    fc.update_max_streams(2, false)
        .expect("uni の増加は受理されなければならない");
    assert!(fc.can_open_uni_stream(), "uni の開設可否が変わる");
    assert_eq!(fc.max_streams_uni_remote(), 2);
    assert_eq!(fc.max_streams_bidi_remote(), 3, "bidi のピア上限は不変");

    // uni の同値拒否で bidi の上限も開設可否も変わらない
    assert!(fc.update_max_streams(2, false).is_err());
    assert_eq!(fc.max_streams_bidi_remote(), 3);
    assert_eq!(fc.max_streams_uni_remote(), 2);
}

/// ローカル広告 API が 2^60 と厳密増加を検証し、成功時にローカル上限を更新すること
///
/// draft-ietf-webtrans-http2-16 Section 6.7。
#[test]
fn test_update_max_streams_local() {
    let mut fc = WtFlowControl::new(65536, 65536, 2, 2, 1, 1);

    // 同値・減少は拒否され、ローカル上限は変わらない
    assert!(fc.update_max_streams_local(2, true).is_err());
    assert!(fc.update_max_streams_local(1, true).is_err());
    assert_eq!(fc.max_streams_bidi_local(), 2);

    // 増加は受理され、ローカル上限に反映される
    fc.update_max_streams_local(3, true)
        .expect("増加する広告は受理されなければならない");
    assert_eq!(fc.max_streams_bidi_local(), 3);
    assert_eq!(fc.max_streams_uni_local(), 1, "uni は不変");
    fc.update_max_streams_local(2, false)
        .expect("uni の増加も受理されなければならない");
    assert_eq!(fc.max_streams_uni_local(), 2);

    // 2^60 ちょうどは受理され、超過は拒否される (上限は 2^60 のまま)
    fc.update_max_streams_local(1u64 << 60, true)
        .expect("2^60 ちょうどは受理されなければならない");
    assert_eq!(fc.max_streams_bidi_local(), 1u64 << 60);
    assert!(fc.update_max_streams_local(1u64 << 60, true).is_err());
    assert_eq!(fc.max_streams_bidi_local(), 1u64 << 60);
    assert!(fc.update_max_streams_local((1u64 << 60) + 1, true).is_err());
    assert_eq!(fc.max_streams_bidi_local(), 1u64 << 60);
}

#[test]
fn test_should_send_max_data() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    assert!(!fc.should_send_max_data(65536));

    fc.consume_recv(40000).expect("should succeed");
    assert!(fc.should_send_max_data(65536));
}

/// `initial_max_data == 1` でも 1 バイト受信後にウィンドウ拡張が必要と
/// 判定されることを確認する (`initial / 2` の切り捨てでしきい値が 0 になり
/// 拡張されない問題の回帰テスト)。
#[test]
fn test_should_send_max_data_initial_one() {
    let mut fc = WtFlowControl::new(1, 1, 100, 100, 50, 50);

    // 初期状態 (recv_available = 1) はしきい値 (div_ceil(2) = 1) 未満ではない
    assert!(!fc.should_send_max_data(1));

    // 1 バイト受信すると recv_available = 0 になり拡張が必要になる
    fc.consume_recv(1).expect("should succeed");
    assert!(fc.should_send_max_data(1));

    // initial_max_data == 0 は従来どおり拡張しない
    assert!(!fc.should_send_max_data(0));
}

/// `initial_max_data` が奇数のときの切り上げしきい値を確認する
/// (`initial = 3` は `div_ceil(2) = 2` となり `recv_available = 1` でも拡張する)。
#[test]
fn test_should_send_max_data_odd_initial() {
    let mut fc = WtFlowControl::new(3, 3, 100, 100, 50, 50);

    // recv_available = 3 はしきい値 2 未満ではない
    assert!(!fc.should_send_max_data(3));

    // recv_available = 2 もしきい値 2 未満ではない
    fc.consume_recv(1).expect("should succeed");
    assert!(!fc.should_send_max_data(3));

    // recv_available = 1 でしきい値 2 未満になり拡張する
    fc.consume_recv(1).expect("should succeed");
    assert!(fc.should_send_max_data(3));
}

/// update_recv_max が varint MAX_VALUE を超えた場合にエラーを返すことを確認する
#[test]
fn test_update_recv_max_varint_overflow() {
    use shiguredo_http2::webtransport::MAX_VALUE;

    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);
    fc.update_recv_max(MAX_VALUE)
        .expect("増加する上限は受理されなければならない");
    assert_eq!(fc.recv_max(), MAX_VALUE);

    // varint 上限を超える値はエラー
    assert!(fc.update_recv_max(MAX_VALUE + 1).is_err());
    assert_eq!(
        fc.recv_max(),
        MAX_VALUE,
        "拒否時に受信上限が変わってはいけない"
    );
}

/// update_recv_max が同値と減少を拒否することを確認する
/// (draft-ietf-webtrans-http2-16 Section 6.5)
#[test]
fn test_update_recv_max_requires_strict_increase() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    assert!(fc.update_recv_max(65536).is_err(), "同値は拒否されるはず");
    assert!(fc.update_recv_max(1).is_err(), "減少は拒否されるはず");
    assert_eq!(fc.recv_max(), 65536, "拒否時に受信上限が変わってはいけない");

    fc.update_recv_max(65537).expect("増加は受理されるはず");
    assert_eq!(fc.recv_max(), 65537);
}

/// update_max_streams で 2^60 を超える値はエラーになることを確認する
/// (draft-ietf-webtrans-http2-16 Section 6.7)
#[test]
fn test_update_max_streams_boundary_errors() {
    let mut fc = WtFlowControl::new(65536, 65536, 100, 100, 50, 50);

    // 2^60 ちょうどは成功し、上限が 2^60 のままになる
    fc.update_max_streams(1u64 << 60, true)
        .expect("2^60 は成功すること");
    assert_eq!(fc.max_streams_bidi_remote(), 1u64 << 60);

    // 到達後の同値は拒否される
    assert!(
        fc.update_max_streams(1u64 << 60, true).is_err(),
        "2^60 到達後の同値は拒否されなければならない"
    );

    // 2^60 + 1 はエラー
    assert!(fc.update_max_streams((1u64 << 60) + 1, true).is_err());
    assert!(fc.update_max_streams((1u64 << 60) + 1, false).is_err());
}
