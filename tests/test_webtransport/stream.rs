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

/// update_recv_max が varint MAX_VALUE を超えた場合にエラーを返すことを確認する
#[test]
fn test_update_recv_max_varint_overflow() {
    use shiguredo_http2::webtransport::MAX_VALUE;

    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    // MAX_VALUE ちょうど → 成功
    stream
        .update_recv_max(MAX_VALUE)
        .expect("MAX_VALUE は成功すること");
    assert_eq!(stream.recv_available(), MAX_VALUE);

    // MAX_VALUE + 1 → エラー
    assert!(stream.update_recv_max(MAX_VALUE + 1).is_err());
}

/// DataRecvd (FIN 送信済み) 後の send_data はエラーになることを確認する
#[test]
fn test_send_data_after_fin_is_error() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    // FIN 付きで送信 → DataRecvd に遷移
    stream
        .send_data(100, true)
        .expect("operation should succeed");
    assert_eq!(stream.send_state(), SendState::DataRecvd);

    // DataRecvd 後の send_data はエラー
    assert!(stream.send_data(100, false).is_err());
}

/// DataRecvd (FIN 受信済み) 後の recv_data はエラーになることを確認する
#[test]
fn test_recv_data_after_fin_is_error() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    // FIN 付きで受信 → DataRecvd に遷移
    stream
        .recv_data(100, true)
        .expect("operation should succeed");
    assert_eq!(stream.recv_state(), RecvState::DataRecvd);

    // DataRecvd 後の recv_data はエラー
    assert!(stream.recv_data(100, false).is_err());
}

/// ResetRead (リセット受信済み) 後の recv_data はエラーになることを確認する
#[test]
fn test_recv_data_after_reset_is_error() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    // リセット受信 → ResetRead に遷移
    stream.recv_reset();
    assert_eq!(stream.recv_state(), RecvState::ResetRead);

    // ResetRead 後の recv_data はエラー
    assert!(stream.recv_data(100, false).is_err());
}

/// ResetRecvd (リセット送信済み) 後の send_data はエラーになることを確認する
#[test]
fn test_send_data_after_reset_is_error() {
    let mut stream = WtStream::new(0, 65536, 65536, true, true);

    // リセット送信 → ResetRecvd に遷移
    stream.send_reset();
    assert_eq!(stream.send_state(), SendState::ResetRecvd);

    // ResetRecvd 後の send_data はエラー
    assert!(stream.send_data(100, false).is_err());
}

/// 受信パートの有無がストリームの方向 (双方向 / 開始主体) で決まることを確認する。
///
/// draft-ietf-webtrans-http2-15 Section 6.2 / Section 6.9: 送信専用ストリームへの
/// WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED の拒否に使う判定。
#[test]
fn test_has_recv_part_by_direction() {
    // ローカルをクライアントとした場合のストリーム ID 割り当て
    // (0 = ローカル開始 bidi、1 = ピア開始 bidi、2 = ローカル開始 uni、3 = ピア開始 uni)

    // ローカル開始 bidi: 受信パートあり
    assert!(
        WtStream::new(0, 0, 0, true, true).has_recv_part(),
        "ローカル開始 bidi は受信パートを持つはず"
    );
    // ピア開始 bidi: 受信パートあり
    assert!(
        WtStream::new(1, 0, 0, true, false).has_recv_part(),
        "ピア開始 bidi は受信パートを持つはず"
    );
    // ローカル開始 uni (送信専用): 受信パートなし
    assert!(
        !WtStream::new(2, 0, 0, false, true).has_recv_part(),
        "ローカル開始 uni は受信パートを持たないはず"
    );
    // ピア開始 uni (受信専用): 受信パートあり
    assert!(
        WtStream::new(3, 0, 0, false, false).has_recv_part(),
        "ピア開始 uni は受信パートを持つはず"
    );
}

/// 送信パートの有無がストリームの方向 (双方向 / 開始主体) で決まることを確認する。
///
/// RFC 9000 Section 2.1 / Section 19.5 / Section 19.10: 受信専用ストリームへの
/// 送信系操作と WT_STOP_SENDING / WT_MAX_STREAM_DATA の拒否に使う判定。
#[test]
fn test_has_send_part_by_direction() {
    // ローカルをクライアントとした場合のストリーム ID 割り当て
    // (0 = ローカル開始 bidi、1 = ピア開始 bidi、2 = ローカル開始 uni、3 = ピア開始 uni)

    // ローカル開始 bidi: 送信パートあり
    assert!(
        WtStream::new(0, 0, 0, true, true).has_send_part(),
        "ローカル開始 bidi は送信パートを持つはず"
    );
    // ピア開始 bidi: 送信パートあり
    assert!(
        WtStream::new(1, 0, 0, true, false).has_send_part(),
        "ピア開始 bidi は送信パートを持つはず"
    );
    // ローカル開始 uni (送信専用): 送信パートあり
    assert!(
        WtStream::new(2, 0, 0, false, true).has_send_part(),
        "ローカル開始 uni は送信パートを持つはず"
    );
    // ピア開始 uni (受信専用): 送信パートなし
    assert!(
        !WtStream::new(3, 0, 0, false, false).has_send_part(),
        "ピア開始 uni は送信パートを持たないはず"
    );
}
