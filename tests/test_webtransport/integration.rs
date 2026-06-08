use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, CapsuleEncoder, WtConfig, WtEvent, WtSession, WtStreamId,
    stream::stream_id as wt_stream_id,
};

/// 1 つの Capsule をデコードするヘルパー
fn decode_single_capsule(bytes: &[u8]) -> Capsule {
    let mut decoder = CapsuleDecoder::new();
    decoder.feed(bytes);
    decoder.decode().expect("decode").expect("capsule expected")
}

/// `grow_recv_window` が `WT_MAX_DATA` capsule をエンコードする
#[test]
fn grow_recv_window_emits_wt_max_data() {
    let mut session = WtSession::server(WtConfig::default());
    session.initiate().unwrap();

    session.grow_recv_window(65_536).unwrap();
    let out = session.poll_output().expect("output expected");

    let capsule = decode_single_capsule(&out);
    match capsule {
        Capsule::WtMaxData { maximum } => {
            // default.initial_max_data = 1_048_576、increment = 65_536
            assert_eq!(maximum, 1_048_576 + 65_536);
        }
        other => panic!("expected WtMaxData, got {other:?}"),
    }
}

/// `grow_stream_recv_window` で存在しない stream_id を指定すると `invalid_stream_id`
#[test]
fn grow_stream_recv_window_unknown_stream_errors() {
    let mut session = WtSession::server(WtConfig::default());
    session.initiate().unwrap();

    let err = session.grow_stream_recv_window(0, 1024).unwrap_err();
    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::InvalidStreamId,
        "unexpected error kind: {err:?}"
    );
}

/// `grow_max_streams(bidi=true)` が `WT_MAX_STREAMS (bidirectional)` を出力する
#[test]
fn grow_max_streams_bidi_emits_capsule() {
    let config = WtConfig {
        initial_max_streams_bidi: 4,
        ..WtConfig::default()
    };
    let mut session = WtSession::server(config);
    session.initiate().unwrap();

    session.grow_max_streams(4, true).unwrap();
    let out = session.poll_output().expect("output expected");
    let capsule = decode_single_capsule(&out);

    match capsule {
        Capsule::WtMaxStreams {
            maximum,
            bidirectional,
        } => {
            assert!(bidirectional);
            assert_eq!(maximum, 4 + 4);
        }
        other => panic!("expected WtMaxStreams bidi, got {other:?}"),
    }
}

/// ピア側から受信した `WT_MAX_DATA` が減少値ならセッションエラー
#[test]
fn received_wt_max_data_decrease_errors() {
    // initial_max_data = 1024 のセッションに対し、500 という減少値を送り込む
    let config = WtConfig {
        initial_max_data: 1024,
        ..WtConfig::default()
    };
    let mut session = WtSession::server(config);
    session.initiate().unwrap();

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtMaxData { maximum: 500 });
    let bytes = encoder.take();

    session.feed(&bytes).unwrap();
    let err = session.process().unwrap_err();
    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
}

/// `close` 後の送信系操作はエラーになる
#[test]
fn send_after_close_errors() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let bidi_id = session.open_bidi_stream().unwrap();
    session.close(0, "bye").unwrap();

    // draft-ietf-webtrans-http2-14 Section 6.12: WT_CLOSE_SESSION 送信後は END_STREAM で half-close するため送信不可
    let err = session.send_stream_data(bidi_id, b"x", false).unwrap_err();
    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::SessionStateError
    );

    let err = session.send_datagram(b"x").unwrap_err();
    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::SessionStateError
    );
}

/// `open_bidi_stream` がローカルのストリーム上限で `flow_control_error`
#[test]
fn open_bidi_stream_over_limit_errors() {
    let config = WtConfig {
        initial_max_streams_bidi: 1,
        ..WtConfig::default()
    };
    let mut session = WtSession::client(config);
    session.initiate().unwrap();

    let _ = session.open_bidi_stream().unwrap();
    let err = session.open_bidi_stream().unwrap_err();
    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
}

/// `send_max_data` の直接呼び出しで capsule が出力される
#[test]
fn send_max_data_emits_capsule() {
    let mut session = WtSession::server(WtConfig::default());
    session.initiate().unwrap();

    session.send_max_data(9_999_999).unwrap();
    let out = session.poll_output().expect("output expected");
    let capsule = decode_single_capsule(&out);
    match capsule {
        Capsule::WtMaxData { maximum } => assert_eq!(maximum, 9_999_999),
        other => panic!("expected WtMaxData, got {other:?}"),
    }
}

/// peer 起点の bidi ストリーム到着後、`stream()` と `flow_control()` getter が動作する
#[test]
fn getters_return_expected_state() {
    let mut session = WtSession::server(WtConfig::default());
    session.initiate().unwrap();

    // クライアント (peer) 起点の bidi ストリーム (id=0) から `hi` を受信した扱いにする
    let peer_id: WtStreamId = wt_stream_id::first(true, true);
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: b"hi".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).unwrap();
    session.process().unwrap();

    // StreamOpened と StreamData の 2 イベントが発火している想定
    let mut got_opened = false;
    let mut got_data = false;
    while let Some(ev) = session.poll_event() {
        match ev {
            WtEvent::StreamOpened { stream_id, .. } if stream_id == peer_id => got_opened = true,
            WtEvent::StreamData {
                stream_id, data, ..
            } if stream_id == peer_id => {
                assert_eq!(data, b"hi");
                got_data = true;
            }
            _ => {}
        }
    }
    assert!(got_opened);
    assert!(got_data);

    // getter の動作確認
    let stream = session.stream(peer_id).expect("stream must exist");
    assert_eq!(stream.recv_offset(), 2);
    assert!(stream.is_bidirectional());

    let fc = session.flow_control();
    assert_eq!(fc.recv_offset(), 2);

    let cfg = session.config();
    assert_eq!(cfg.initial_max_data, WtConfig::default().initial_max_data);
}

/// Ready 状態のストリームに WT_STOP_SENDING を受信すると
/// WT_RESET_STREAM が自動応答されることを確認する。
#[test]
fn stop_sending_triggers_auto_reset_ready_state() {
    // サーバー側のピア (client) が開いた bidi ストリーム (id=0) に対して
    // サーバーが WT_STOP_SENDING を送り、ピアが WT_RESET_STREAM で応答するケースを模擬する。
    // ここではサーバーが stop_sending を送信した扱いでテストする。
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    // クライアント側で bidi ストリームを開く (id=0, Ready → このストリームはピアから見て Ready)
    let stream_id = session.open_bidi_stream().unwrap();

    // ピア (サーバー) から WT_STOP_SENDING を受信
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 42,
    });
    session.feed(&encoder.take()).unwrap();
    session.process().unwrap();

    // WT_RESET_STREAM が出力バッファに含まれていることを確認
    assert!(session.has_output());

    let out = session.poll_output().expect("output expected");
    let capsule = decode_single_capsule(&out);
    match capsule {
        Capsule::WtResetStream {
            stream_id: sid,
            error_code,
            ..
        } => {
            assert_eq!(sid, stream_id);
            assert_eq!(error_code, 42);
        }
        other => panic!("expected WtResetStream, got {other:?}"),
    }
}

/// Send 状態のストリームに WT_STOP_SENDING を受信すると
/// WT_RESET_STREAM が自動応答されることを確認する。
#[test]
fn stop_sending_triggers_auto_reset_send_state() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let stream_id = session.open_bidi_stream().unwrap();

    // 送信して Send 状態に遷移させる
    session
        .send_stream_data(stream_id, b"hello", false)
        .unwrap();

    // 出力を消費してから WT_STOP_SENDING を受信
    while session.poll_output().is_some() {}

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 99,
    });
    session.feed(&encoder.take()).unwrap();
    session.process().unwrap();

    assert!(session.has_output());

    let out = session.poll_output().expect("output expected");
    let capsule = decode_single_capsule(&out);
    match capsule {
        Capsule::WtResetStream {
            stream_id: sid,
            error_code,
            ..
        } => {
            assert_eq!(sid, stream_id);
            assert_eq!(error_code, 99);
        }
        other => panic!("expected WtResetStream, got {other:?}"),
    }
}

/// DataSent 状態のストリームでは WT_STOP_SENDING 受信時に
/// WT_RESET_STREAM が生成されないことを確認する。
#[test]
fn stop_sending_no_auto_reset_data_sent_state() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let stream_id = session.open_bidi_stream().unwrap();
    session.send_stream_data(stream_id, b"done", true).unwrap();

    // 出力を消費
    while session.poll_output().is_some() {}

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 1,
    });
    session.feed(&encoder.take()).unwrap();
    session.process().unwrap();

    // DataSent では WT_RESET_STREAM が生成されない
    assert!(!session.has_output());

    // ただし WtEvent::StopSending は発行される
    let mut got_stop_sending = false;
    while let Some(ev) = session.poll_event() {
        if matches!(
            ev,
            WtEvent::StopSending {
                stream_id: sid,
                error_code,
            } if sid == stream_id && error_code == 1
        ) {
            got_stop_sending = true;
        }
    }
    assert!(got_stop_sending);
}

/// 存在しないストリーム ID への WT_STOP_SENDING はエラーにならず
/// WtEvent::StopSending を発行することを確認する。
#[test]
fn stop_sending_unknown_stream_emits_event() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id: 9999,
        error_code: 0,
    });
    session.feed(&encoder.take()).unwrap();
    session.process().unwrap();

    assert!(!session.has_output());

    let mut got_stop_sending = false;
    while let Some(ev) = session.poll_event() {
        if matches!(ev, WtEvent::StopSending { .. }) {
            got_stop_sending = true;
        }
    }
    assert!(got_stop_sending);
}

/// 重複 WT_STOP_SENDING 受信は stream_state_error になることを確認する。
#[test]
fn stop_sending_duplicate_errors() {
    let mut session = WtSession::client(WtConfig::default());
    session.initiate().unwrap();

    let stream_id = session.open_bidi_stream().unwrap();

    // 1 回目の WT_STOP_SENDING
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 0,
    });
    session.feed(&encoder.take()).unwrap();
    session.process().unwrap();

    // 出力消費
    while session.poll_output().is_some() {}
    while session.poll_event().is_some() {}

    // 2 回目の WT_STOP_SENDING はエラー
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 0,
    });
    session.feed(&encoder.take()).unwrap();
    let err = session.process().unwrap_err();
    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
}

/// 未登録ストリームへの WT_RESET_STREAM が stream_state_error を返すことを確認する。
#[test]
fn wt_reset_stream_unknown_stream_id_errors() {
    let mut session = WtSession::server(WtConfig::default());
    session.initiate().unwrap();

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: 0,
        error_code: 1,
        reliable_size: 0,
    });
    session.feed(&encoder.take()).unwrap();
    let err = session.process().unwrap_err();

    assert_eq!(
        err.kind,
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason.contains("unknown stream"));
}
