use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, CapsuleEncoder, MAX_VALUE, WtConfig, WtEvent, WtSession, WtStreamId,
    stream::{SendState, stream_id as wt_stream_id},
};

/// 1 つの Capsule をデコードするヘルパー
fn decode_single_capsule(bytes: &[u8]) -> Capsule {
    let mut decoder = CapsuleDecoder::new();
    decoder.feed(bytes).expect("feed should succeed");
    decoder.decode().expect("decode").expect("capsule expected")
}

/// `grow_recv_window` が `WT_MAX_DATA` capsule をエンコードする
#[test]
fn grow_recv_window_emits_wt_max_data() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    session
        .grow_recv_window(65_536)
        .expect("initiate should succeed");
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
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let err = session.grow_stream_recv_window(0, 1024).unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::InvalidStreamId,
        "unexpected error kind: {err:?}"
    );
}

/// `WT_STOP_SENDING` 送信後の `send_max_stream_data` / `grow_stream_recv_window` は
/// `stream_state_error` になること (draft-ietf-webtrans-http2-15 Section 6.6)
#[test]
fn send_max_stream_data_after_stop_sending_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("open stream");
    session
        .stop_sending(stream_id, 0)
        .expect("stop_sending should succeed");
    // stop_sending の出力を捨てる
    let _ = session.poll_output();

    let err = session
        .send_max_stream_data(stream_id, 1_000_000)
        .expect_err("STOP_SENDING 後の MAX_STREAM_DATA は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError,
        "unexpected error kind: {err}"
    );

    let err = session
        .grow_stream_recv_window(stream_id, 1024)
        .expect_err("STOP_SENDING 後の grow も拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError,
        "unexpected error kind: {err}"
    );
}

/// `grow_max_streams(bidi=true)` が `WT_MAX_STREAMS (bidirectional)` を出力する
#[test]
fn grow_max_streams_bidi_emits_capsule() {
    let config = WtConfig {
        initial_max_streams_bidi: 4,
        ..WtConfig::default()
    };
    let mut session = WtSession::server(config.clone(), config);
    session.initiate().expect("initiate should succeed");

    session
        .grow_max_streams(4, true)
        .expect("initiate should succeed");
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
/// (draft-ietf-webtrans-http2-15 Section 6.5: 減少値の受信時は WT_FLOW_CONTROL_ERROR でセッションを閉じなければならない (MUST))
#[test]
fn received_wt_max_data_decrease_errors() {
    // initial_max_data = 1024 のセッションに対し、500 という減少値を送り込む
    let config = WtConfig {
        initial_max_data: 1024,
        ..WtConfig::default()
    };
    let mut session = WtSession::server(config.clone(), config);
    session.initiate().expect("initiate should succeed");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtMaxData { maximum: 500 });
    let bytes = encoder.take();

    session.feed(&bytes).expect("feed should succeed");
    let err = session.process().unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
}

/// `close` 後の送信系操作はエラーになる
#[test]
fn send_after_close_errors() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let bidi_id = session.open_bidi_stream().expect("initiate should succeed");
    session.close(0, "bye").expect("open stream should succeed");

    // draft-ietf-webtrans-http2-15 Section 6.12: WT_CLOSE_SESSION 送信後は END_STREAM で half-close するため送信不可
    let err = session.send_stream_data(bidi_id, b"x", false).unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::SessionStateError
    );

    let err = session.send_datagram(b"x").unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::SessionStateError
    );
}

/// 存在しないストリーム ID への WT_STREAM_DATA_BLOCKED で WT_STREAM_STATE_ERROR が返ることを確認する
/// (draft-ietf-webtrans-http2-15 Section 6.9)
#[test]
fn wt_stream_data_blocked_unknown_stream_errors() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // 存在しないストリーム ID への WT_STREAM_DATA_BLOCKED
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStreamDataBlocked {
        stream_id: 999,
        maximum: 1024,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    let err = session.process().unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("unknown stream"));
}

/// 受信側が終端状態のストリームに WT_STREAM_DATA_BLOCKED を受信した場合に
/// WT_STREAM_STATE_ERROR が返ることを確認する
/// (draft-ietf-webtrans-http2-15 Section 6.9)
#[test]
fn wt_stream_data_blocked_recv_terminal_state_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // クライアント開始 bidi ストリーム (ID=0) をピアが開設し FIN 付きで送信
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: 0,
        data: b"hello".to_vec(),
        fin: true,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // poll_event で StreamData { fin: true } を pop して DataRead に遷移させる
    let mut found_fin = false;
    while let Some(event) = session.poll_event() {
        if let WtEvent::StreamData { fin: true, .. } = event {
            found_fin = true;
            break;
        }
    }
    assert!(found_fin, "FIN 付き StreamData イベントが取得できること");

    // 受信側が終端状態のストリームに WT_STREAM_DATA_BLOCKED を送信
    let mut encoder2 = CapsuleEncoder::new();
    encoder2.encode(&Capsule::WtStreamDataBlocked {
        stream_id: 0,
        maximum: 1024,
    });
    session.feed(&encoder2.take()).expect("feed should succeed");
    let err = session.process().unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("not in valid state"));
}

/// `open_bidi_stream` がローカルのストリーム上限で `flow_control_error`
/// (draft-ietf-webtrans-http2-15 Section 6.7: 現在のストリーム上限を超えてストリームを開いてはならない (MUST NOT))
#[test]
fn open_bidi_stream_over_limit_errors() {
    let config = WtConfig {
        initial_max_streams_bidi: 1,
        ..WtConfig::default()
    };
    let mut session = WtSession::client(config.clone(), config);
    session.initiate().expect("initiate should succeed");

    let _ = session.open_bidi_stream().expect("initiate should succeed");
    let err = session.open_bidi_stream().unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
}

/// `send_max_data` の直接呼び出しで capsule が出力される
#[test]
fn send_max_data_emits_capsule() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    session
        .send_max_data(9_999_999)
        .expect("initiate should succeed");
    let out = session.poll_output().expect("output expected");
    let capsule = decode_single_capsule(&out);
    match capsule {
        Capsule::WtMaxData { maximum } => assert_eq!(maximum, 9_999_999),
        other => panic!("expected WtMaxData, got {other:?}"),
    }
}

/// アプリケーションエラーコードの最大値 (draft-ietf-webtrans-http2-15 Section 6.2 / 6.3)
const MAX_APP_ERROR_CODE: u64 = 0xffff_ffff;

/// `send_max_data` が varint 上限 (2^62-1) を超える値を拒否することを確認する
///
/// varint でエンコードできない値は CapsuleEncoder 内で panic するため、
/// 公開 API が事前にエラーを返す必要がある (RFC 9000 Section 16)。
#[test]
fn send_max_data_rejects_varint_overflow() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate は成功すること");

    // 32-bit 上限 (0xffffffff) を超えるが varint 上限以下の値は成功する
    // (WT_MAX_DATA の Maximum は varint 制約のみ。RFC 9000 Section 16)
    session
        .send_max_data(0x1_0000_0000)
        .expect("0xffffffff 超は varint 制約のみで拒否されないこと");
    let _ = session.poll_output();

    // 上限ちょうどは成功し、8 バイト varint として往復できる
    session
        .send_max_data(MAX_VALUE)
        .expect("MAX_VALUE は成功すること");
    let out = session.poll_output().expect("出力が得られること");
    let capsule = decode_single_capsule(&out);
    match capsule {
        Capsule::WtMaxData { maximum } => assert_eq!(maximum, MAX_VALUE),
        other => panic!("WtMaxData を期待したが、実際は {other:?}"),
    }

    // 上限 + 1 は flow_control_error で拒否される (panic しない)
    let err = session
        .send_max_data(MAX_VALUE + 1)
        .expect_err("varint 上限超過はエラーになること");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError,
        "予期しないエラー種別: {err}"
    );
}

/// `send_max_stream_data` が varint 上限 (2^62-1) を超える値を拒否することを確認する
#[test]
fn send_max_stream_data_rejects_varint_overflow() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate は成功すること");
    let stream_id = session.open_bidi_stream().expect("ストリームを開けること");

    // 32-bit 上限 (0xffffffff) を超えるが varint 上限以下の値は成功する
    session
        .send_max_stream_data(stream_id, 0x1_0000_0000)
        .expect("0xffffffff 超は varint 制約のみで拒否されないこと");

    // 上限ちょうどは成功する
    session
        .send_max_stream_data(stream_id, MAX_VALUE)
        .expect("MAX_VALUE は成功すること");

    // 上限 + 1 は flow_control_error で拒否される (panic しない)
    let err = session
        .send_max_stream_data(stream_id, MAX_VALUE + 1)
        .expect_err("varint 上限超過はエラーになること");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError,
        "予期しないエラー種別: {err}"
    );
}

/// `reset_stream` が 0xffffffff を超える error_code を拒否することを確認する
///
/// draft-ietf-webtrans-http2-15 Section 6.2: error_code は 0xffffffff 以下でなければ
/// ならない (MUST NOT)。0xffffffff 超は仕様違反であり、varint 上限 (2^62-1) 超では
/// CapsuleEncoder 内で panic するため、事前に拒否する。
#[test]
fn reset_stream_rejects_error_code_overflow() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate は成功すること");

    // 上限ちょうど (0xffffffff) は成功する
    let stream_id = session.open_bidi_stream().expect("ストリームを開けること");
    session
        .reset_stream(stream_id, MAX_APP_ERROR_CODE)
        .expect("0xffffffff は成功すること");

    // 上限 + 1 (仕様違反だが varint で表現可能) は flow_control_error で拒否される
    let stream_id = session.open_bidi_stream().expect("ストリームを開けること");
    let err = session
        .reset_stream(stream_id, MAX_APP_ERROR_CODE + 1)
        .expect_err("0xffffffff 超過はエラーになること");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError,
        "予期しないエラー種別: {err}"
    );

    // varint 上限 (2^62-1) を超える値も同じ 0xffffffff チェックで拒否される
    // (エンコード時に CapsuleEncoder 内で panic する値のため、事前に拒否する)
    let stream_id = session.open_bidi_stream().expect("ストリームを開けること");
    let err = session
        .reset_stream(stream_id, u64::MAX)
        .expect_err("varint 上限超過はエラーになること");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError,
        "予期しないエラー種別: {err}"
    );
}

/// `stop_sending` が 0xffffffff を超える error_code を拒否することを確認する
///
/// draft-ietf-webtrans-http2-15 Section 6.3: error_code は 0xffffffff 以下でなければ
/// ならない (MUST NOT)。0xffffffff 超は仕様違反であり、varint 上限 (2^62-1) 超では
/// CapsuleEncoder 内で panic するため、事前に拒否する。
#[test]
fn stop_sending_rejects_error_code_overflow() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate は成功すること");

    // 上限ちょうど (0xffffffff) は成功する
    let stream_id = session.open_bidi_stream().expect("ストリームを開けること");
    session
        .stop_sending(stream_id, MAX_APP_ERROR_CODE)
        .expect("0xffffffff は成功すること");

    // 上限 + 1 (仕様違反だが varint で表現可能) は flow_control_error で拒否される
    let stream_id = session.open_bidi_stream().expect("ストリームを開けること");
    let err = session
        .stop_sending(stream_id, MAX_APP_ERROR_CODE + 1)
        .expect_err("0xffffffff 超過はエラーになること");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError,
        "予期しないエラー種別: {err}"
    );

    // varint 上限 (2^62-1) を超える値も同じ 0xffffffff チェックで拒否される
    // (エンコード時に CapsuleEncoder 内で panic する値のため、事前に拒否する)
    let stream_id = session.open_bidi_stream().expect("ストリームを開けること");
    let err = session
        .stop_sending(stream_id, u64::MAX)
        .expect_err("varint 上限超過はエラーになること");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError,
        "予期しないエラー種別: {err}"
    );
}

/// peer 起点の bidi ストリーム到着後、`stream()` と `flow_control()` getter が動作する
#[test]
fn getters_return_expected_state() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // クライアント (peer) 起点の bidi ストリーム (id=0) から `hi` を受信した扱いにする
    let peer_id: WtStreamId = wt_stream_id::first(true, true);
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: b"hi".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

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
/// (draft-ietf-webtrans-http2-15 Section 6.3: 受信者はストリームが Ready または Send 状態の場合、同一エラーコードの WT_RESET_STREAM で応答する)
#[test]
fn stop_sending_triggers_auto_reset_ready_state() {
    // サーバー側のピア (client) が開いた bidi ストリーム (id=0) に対して
    // サーバーが WT_STOP_SENDING を送り、ピアが WT_RESET_STREAM で応答するケースを模擬する。
    // ここではサーバーが stop_sending を送信した扱いでテストする。
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // クライアント側で bidi ストリームを開く (id=0, Ready → このストリームはピアから見て Ready)
    let stream_id = session
        .open_bidi_stream()
        .expect("open stream should succeed");

    // ピア (サーバー) から WT_STOP_SENDING を受信
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 42,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

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
/// (draft-ietf-webtrans-http2-15 Section 6.3: 受信者はストリームが Ready または Send 状態の場合、同一エラーコードの WT_RESET_STREAM で応答する)
#[test]
fn stop_sending_triggers_auto_reset_send_state() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("initiate should succeed");

    // 送信して Send 状態に遷移させる
    session
        .send_stream_data(stream_id, b"hello", false)
        .expect("operation should succeed");

    // 出力を消費してから WT_STOP_SENDING を受信
    while session.poll_output().is_some() {}

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 99,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

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

/// FIN 送信済み (DataRecvd) のストリームでは WT_STOP_SENDING 受信時に
/// WT_RESET_STREAM が生成されないことを確認する。
/// (draft-ietf-webtrans-http2-15 Section 6.3: 受信者はストリームが Ready または Send 状態の場合、同一エラーコードの WT_RESET_STREAM で応答する)
#[test]
fn stop_sending_no_auto_reset_after_fin_sent() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("initiate should succeed");
    session
        .send_stream_data(stream_id, b"done", true)
        .expect("operation should succeed");

    // 出力を消費
    while session.poll_output().is_some() {}

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 1,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // FIN 送信済み (DataRecvd) では WT_RESET_STREAM が生成されない
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
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id: 9999,
        error_code: 0,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

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
/// (draft-ietf-webtrans-http2-15 Section 6.3: 2 回目の WT_STOP_SENDING 受信時は WT_STREAM_STATE_ERROR のストリームエラーを送らなければならない (MUST))
#[test]
fn stop_sending_duplicate_errors() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("initiate should succeed");

    // 1 回目の WT_STOP_SENDING
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 0,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // 出力消費
    while session.poll_output().is_some() {}
    while session.poll_event().is_some() {}

    // 2 回目の WT_STOP_SENDING はエラー
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id,
        error_code: 0,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    let err = session.process().unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
}

/// 未登録ストリームへの WT_RESET_STREAM が stream_state_error を返すことを確認する。
/// (draft-ietf-webtrans-http2-15 Section 6.2: 有効な状態にないストリームへの WT_RESET_STREAM 受信時は WT_STREAM_STATE_ERROR のストリームエラーを送らなければならない (MUST))
#[test]
fn wt_reset_stream_unknown_stream_id_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: 0,
        error_code: 1,
        reliable_size: 0,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    let err = session.process().unwrap_err();

    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("unknown stream"));
}

// draft-ietf-webtrans-http2-15 Section 6.2: Reliable Size は送信済み総量と
// 一致しなければならない (MUST equal)。過小・過大いずれもセッションエラー。

/// reliable_size == recv_offset で WT_RESET_STREAM が正常に処理される
#[test]
fn wt_reset_stream_reliable_size_exact_match() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // クライアント起点の bidi ストリーム (id=0) から 5 バイト受信
    let peer_id: WtStreamId = wt_stream_id::first(true, true);
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: b"hello".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // reliable_size == recv_offset (5) → 正常
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: peer_id,
        error_code: 0,
        reliable_size: 5,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session
        .process()
        .expect("reliable_size == recv_offset should succeed");

    // StreamReset イベントが発火している
    let mut got_reset = false;
    while let Some(ev) = session.poll_event() {
        if let WtEvent::StreamReset { stream_id, .. } = ev
            && stream_id == peer_id
        {
            got_reset = true;
        }
    }
    assert!(got_reset, "StreamReset event expected");
}

/// reliable_size == 0 && recv_offset == 0 で WT_RESET_STREAM が正常に処理される
#[test]
fn wt_reset_stream_reliable_size_zero_match() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // クライアント起点の bidi ストリーム (id=0) をデータなしで開く
    let peer_id: WtStreamId = wt_stream_id::first(true, true);
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: vec![],
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // reliable_size == recv_offset (0) → 正常
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: peer_id,
        error_code: 0,
        reliable_size: 0,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session
        .process()
        .expect("reliable_size == 0 == recv_offset should succeed");
}

/// reliable_size > recv_offset (過大) でセッションエラーになる
#[test]
fn wt_reset_stream_reliable_size_too_large_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // クライアント起点の bidi ストリーム (id=0) から 5 バイト受信
    let peer_id: WtStreamId = wt_stream_id::first(true, true);
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: b"hello".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // reliable_size = 6 > recv_offset = 5 → エラー
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: peer_id,
        error_code: 0,
        reliable_size: 6,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    let err = session.process().unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("does not match"));
}

/// reliable_size < recv_offset (過小) でセッションエラーになる
#[test]
fn wt_reset_stream_reliable_size_too_small_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // クライアント起点の bidi ストリーム (id=0) から 5 バイト受信
    let peer_id: WtStreamId = wt_stream_id::first(true, true);
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: b"hello".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // reliable_size = 4 < recv_offset = 5 → エラー
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: peer_id,
        error_code: 0,
        reliable_size: 4,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    let err = session.process().unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("does not match"));
}

/// 送信専用のローカル開始 uni ストリームへの WT_RESET_STREAM は
/// WT_STREAM_STATE_ERROR になり、StreamReset イベントが送出されないこと。
/// (draft-ietf-webtrans-http2-15 Section 6.2 / RFC 9000 Section 19.4)
#[test]
fn wt_reset_stream_local_uni_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let local_id = session
        .open_uni_stream()
        .expect("uni ストリームを開けるはず");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: local_id,
        error_code: 0,
        reliable_size: 0,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    let err = session
        .process()
        .expect_err("送信専用ストリームへの WT_RESET_STREAM は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("not in valid state"));

    // StreamReset イベントが送出されないこと
    while let Some(ev) = session.poll_event() {
        assert!(
            !matches!(ev, WtEvent::StreamReset { .. }),
            "送信専用ストリームへの WT_RESET_STREAM で StreamReset を送出してはいけない"
        );
    }
}

/// 送信専用のローカル開始 uni ストリームへの WT_STREAM_DATA_BLOCKED は
/// WT_STREAM_STATE_ERROR になること。
/// (draft-ietf-webtrans-http2-15 Section 6.9 / RFC 9000 Section 19.13)
#[test]
fn wt_stream_data_blocked_local_uni_stream_errors() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let local_id = session
        .open_uni_stream()
        .expect("uni ストリームを開けるはず");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStreamDataBlocked {
        stream_id: local_id,
        maximum: 1024,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    let err = session
        .process()
        .expect_err("送信専用ストリームへの WT_STREAM_DATA_BLOCKED は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("not in valid state"));
}

/// ピア開始 uni ストリームは受信パートを持つため、WT_RESET_STREAM が
/// 従来どおり受理され StreamReset イベントが送出されること。
#[test]
fn wt_reset_stream_peer_uni_stream_accepted() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let peer_id = wt_stream_id::first(true, false);

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: b"hi".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    session.process().expect("process に成功するはず");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: peer_id,
        error_code: 0,
        reliable_size: 2,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    session
        .process()
        .expect("ピア開始 uni ストリームは WT_RESET_STREAM を受理するはず");

    let mut got_reset = false;
    while let Some(ev) = session.poll_event() {
        if let WtEvent::StreamReset { stream_id, .. } = ev
            && stream_id == peer_id
        {
            got_reset = true;
        }
    }
    assert!(got_reset, "StreamReset イベントが送出されるはず");
}

/// ローカル開始 bidi ストリームは受信パートを持つため、WT_RESET_STREAM が
/// 従来どおり受理されること。
#[test]
fn wt_reset_stream_local_bidi_stream_accepted() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let local_id = session
        .open_bidi_stream()
        .expect("bidi ストリームを開けるはず");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: local_id,
        error_code: 0,
        reliable_size: 0,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    session
        .process()
        .expect("ローカル開始 bidi ストリームは WT_RESET_STREAM を受理するはず");
}

/// 受信パートを持つストリーム (ローカル開始 bidi / ピア開始 bidi / ピア開始 uni) への
/// WT_STREAM_DATA_BLOCKED は従来どおり受理されること。
#[test]
fn wt_stream_data_blocked_recv_streams_accepted() {
    // ローカル開始 bidi
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let local_id = session
        .open_bidi_stream()
        .expect("bidi ストリームを開けるはず");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStreamDataBlocked {
        stream_id: local_id,
        maximum: 1024,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    session
        .process()
        .expect("ローカル開始 bidi ストリームは WT_STREAM_DATA_BLOCKED を受理するはず");

    // ピア開始 bidi
    let mut session2 = WtSession::server(WtConfig::default(), WtConfig::default());
    session2.initiate().expect("セッションを開始できるはず");
    let peer_bidi_id = wt_stream_id::first(true, true);

    let mut encoder2 = CapsuleEncoder::new();
    encoder2.encode(&Capsule::WtStream {
        stream_id: peer_bidi_id,
        data: b"y".to_vec(),
        fin: false,
    });
    session2
        .feed(&encoder2.take())
        .expect("feed に成功するはず");
    session2.process().expect("process に成功するはず");

    let mut encoder2 = CapsuleEncoder::new();
    encoder2.encode(&Capsule::WtStreamDataBlocked {
        stream_id: peer_bidi_id,
        maximum: 1024,
    });
    session2
        .feed(&encoder2.take())
        .expect("feed に成功するはず");
    session2
        .process()
        .expect("ピア開始 bidi ストリームは WT_STREAM_DATA_BLOCKED を受理するはず");

    // ピア開始 uni
    let mut session3 = WtSession::server(WtConfig::default(), WtConfig::default());
    session3.initiate().expect("セッションを開始できるはず");
    let peer_uni_id = wt_stream_id::first(true, false);

    let mut encoder3 = CapsuleEncoder::new();
    encoder3.encode(&Capsule::WtStream {
        stream_id: peer_uni_id,
        data: b"x".to_vec(),
        fin: false,
    });
    session3
        .feed(&encoder3.take())
        .expect("feed に成功するはず");
    session3.process().expect("process に成功するはず");

    let mut encoder3 = CapsuleEncoder::new();
    encoder3.encode(&Capsule::WtStreamDataBlocked {
        stream_id: peer_uni_id,
        maximum: 1024,
    });
    session3
        .feed(&encoder3.take())
        .expect("feed に成功するはず");
    session3
        .process()
        .expect("ピア開始 uni ストリームは WT_STREAM_DATA_BLOCKED を受理するはず");
}

/// 送信専用のローカル開始 uni ストリームへの WT_STOP_SENDING は
/// 従来どおり受理され、送信停止要求として WT_RESET_STREAM が応答されること。
#[test]
fn wt_stop_sending_local_uni_stream_accepted() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let local_id = session
        .open_uni_stream()
        .expect("uni ストリームを開けるはず");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id: local_id,
        error_code: 7,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    session
        .process()
        .expect("ローカル開始 uni ストリームは WT_STOP_SENDING を受理するはず");

    // 送信停止要求に対して WT_RESET_STREAM が応答される
    let out = session.poll_output().expect("WT_RESET_STREAM の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtResetStream {
            stream_id,
            error_code,
            reliable_size,
        } => {
            assert_eq!(stream_id, local_id);
            assert_eq!(error_code, 7);
            assert_eq!(reliable_size, 0, "未送信なので reliable_size は 0");
        }
        other => panic!("WtResetStream を期待したが {other:?} だった"),
    }
}

/// 送信専用のローカル開始 uni ストリームへの WT_MAX_STREAM_DATA は
/// 従来どおり受理され、送信上限が更新されること。
#[test]
fn wt_max_stream_data_local_uni_stream_accepted() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let local_id = session
        .open_uni_stream()
        .expect("uni ストリームを開けるはず");
    let before = session
        .stream(local_id)
        .expect("ストリームが存在するはず")
        .send_available();

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtMaxStreamData {
        stream_id: local_id,
        maximum: before + 4096,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    session
        .process()
        .expect("ローカル開始 uni ストリームは WT_MAX_STREAM_DATA を受理するはず");

    assert_eq!(
        session
            .stream(local_id)
            .expect("ストリームが存在するはず")
            .send_available(),
        before + 4096,
        "WT_MAX_STREAM_DATA で送信上限が更新されること"
    );
}

// ---- 受信専用ストリーム (ピア開始 uni) の送信系操作の検証 (拒否と非回帰) ----

/// ピア開始ストリームを WT_STREAM で開くヘルパー
fn open_peer_stream(session: &mut WtSession, peer_id: WtStreamId, data: &[u8]) {
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: data.to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    session.process().expect("process に成功するはず");
}

/// 受信専用ストリームへの send_stream_data は stream_state_error になり、
/// ストリーム状態と出力が変化しないこと
/// (draft-ietf-webtrans-http2-15 Section 6.4 / RFC 9000 Section 2.1)
#[test]
fn send_stream_data_receive_only_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let peer_id = wt_stream_id::first(true, false);
    open_peer_stream(&mut session, peer_id, b"x");

    let err = session
        .send_stream_data(peer_id, b"data", false)
        .expect_err("受信専用ストリームへの送信は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("receive-only"));
    let stream = session.stream(peer_id).expect("ストリームが存在するはず");
    assert_eq!(
        stream.send_offset(),
        0,
        "送信済みバイト数が進んではいけない"
    );
    assert_eq!(
        stream.send_state(),
        SendState::Ready,
        "送信状態が変化してはいけない"
    );
    assert!(
        session.poll_output().is_none(),
        "拒否時に出力が生成されてはいけない"
    );
}

/// 受信専用ストリームへの reset_stream は stream_state_error になり、
/// ストリーム状態と出力が変化しないこと
/// (draft-ietf-webtrans-http2-15 Section 6.2 / RFC 9000 Section 19.4)
#[test]
fn reset_stream_receive_only_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let peer_id = wt_stream_id::first(true, false);
    open_peer_stream(&mut session, peer_id, b"x");

    let err = session
        .reset_stream(peer_id, 0)
        .expect_err("受信専用ストリームへのリセットは拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("receive-only"));
    let stream = session.stream(peer_id).expect("ストリームが存在するはず");
    assert_eq!(
        stream.send_offset(),
        0,
        "送信済みバイト数が進んではいけない"
    );
    assert_eq!(
        stream.send_state(),
        SendState::Ready,
        "送信状態が変化してはいけない"
    );
    assert!(
        session.poll_output().is_none(),
        "拒否時に出力が生成されてはいけない"
    );
}

/// 受信専用ストリームへの WT_STOP_SENDING 受信は stream_state_error になり、
/// WT_RESET_STREAM が応答されないこと (RFC 9000 Section 19.5)
#[test]
fn wt_stop_sending_receive_only_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let peer_id = wt_stream_id::first(true, false);
    open_peer_stream(&mut session, peer_id, b"x");
    // 先にストリーム開始イベントを消費しておく
    while session.poll_event().is_some() {}

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStopSending {
        stream_id: peer_id,
        error_code: 7,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    let err = session
        .process()
        .expect_err("受信専用ストリームへの WT_STOP_SENDING は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("receive-only"));
    assert!(
        !session
            .stream(peer_id)
            .expect("ストリームが存在するはず")
            .stop_sending_received(),
        "WT_STOP_SENDING 受信フラグが立ってはいけない"
    );
    assert!(
        session.poll_event().is_none(),
        "イベントが送出されてはいけない"
    );
    assert!(
        session.poll_output().is_none(),
        "WT_RESET_STREAM を応答してはいけない"
    );
}

/// 受信専用ストリームへの WT_MAX_STREAM_DATA 受信は stream_state_error になり、
/// 送信上限が変化しないこと (RFC 9000 Section 19.10)
#[test]
fn wt_max_stream_data_receive_only_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let peer_id = wt_stream_id::first(true, false);
    open_peer_stream(&mut session, peer_id, b"x");
    let before = session
        .stream(peer_id)
        .expect("ストリームが存在するはず")
        .send_available();

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtMaxStreamData {
        stream_id: peer_id,
        maximum: before + 4096,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    let err = session
        .process()
        .expect_err("受信専用ストリームへの WT_MAX_STREAM_DATA は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("receive-only"));
    assert_eq!(
        session
            .stream(peer_id)
            .expect("ストリームが存在するはず")
            .send_available(),
        before,
        "拒否時に送信上限が変化してはいけない"
    );
}

/// 受信専用ストリームへの stop_sending / send_max_stream_data /
/// grow_stream_recv_window は受信側の操作として従来どおり受理されること
#[test]
fn receive_only_stream_recv_operations_accepted() {
    // send_max_stream_data / grow_stream_recv_window の受理
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let peer_id = wt_stream_id::first(true, false);
    open_peer_stream(&mut session, peer_id, b"x");
    let recv_before = session
        .stream(peer_id)
        .expect("ストリームが存在するはず")
        .recv_available();

    // 単調増加になるよう、grow_stream_recv_window が送る recv_max + 4096 より
    // 大きい上限を後で送る
    session
        .grow_stream_recv_window(peer_id, 4096)
        .expect("受信専用ストリームの受信ウィンドウ拡張は成功するはず");
    let _ = session
        .poll_output()
        .expect("WT_MAX_STREAM_DATA の出力が必要");
    assert_eq!(
        session
            .stream(peer_id)
            .expect("ストリームが存在するはず")
            .recv_available(),
        recv_before + 4096,
        "受信ウィンドウが拡張されること"
    );

    session
        .send_max_stream_data(peer_id, 1_000_000)
        .expect("受信専用ストリームへの WT_MAX_STREAM_DATA 送信は成功するはず");
    let _ = session
        .poll_output()
        .expect("WT_MAX_STREAM_DATA の出力が必要");

    // stop_sending の受理
    let mut session2 = WtSession::server(WtConfig::default(), WtConfig::default());
    session2.initiate().expect("セッションを開始できるはず");
    let peer_id2 = wt_stream_id::first(true, false);
    open_peer_stream(&mut session2, peer_id2, b"x");
    session2
        .stop_sending(peer_id2, 7)
        .expect("受信専用ストリームへの WT_STOP_SENDING 送信は成功するはず");
    let out = session2
        .poll_output()
        .expect("WT_STOP_SENDING の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtStopSending {
            stream_id,
            error_code,
        } => {
            assert_eq!(stream_id, peer_id2);
            assert_eq!(error_code, 7);
        }
        other => panic!("WtStopSending を期待したが {other:?} だった"),
    }
}

/// ピア開始 bidi ストリームは送信パートを持つため、WT_MAX_STREAM_DATA 受信・
/// send_stream_data・reset_stream・WT_STOP_SENDING 受信が従来どおり動作すること
#[test]
fn peer_bidi_stream_send_operations_accepted() {
    // WT_MAX_STREAM_DATA 受信と send_stream_data / reset_stream
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let peer_id = wt_stream_id::first(true, true);
    open_peer_stream(&mut session, peer_id, b"hi");

    let before = session
        .stream(peer_id)
        .expect("ストリームが存在するはず")
        .send_available();
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtMaxStreamData {
        stream_id: peer_id,
        maximum: before + 4096,
    });
    session.feed(&encoder.take()).expect("feed に成功するはず");
    session
        .process()
        .expect("ピア開始 bidi への WT_MAX_STREAM_DATA は受理されるはず");
    assert_eq!(
        session
            .stream(peer_id)
            .expect("ストリームが存在するはず")
            .send_available(),
        before + 4096,
        "WT_MAX_STREAM_DATA で送信上限が更新されること"
    );

    session
        .send_stream_data(peer_id, b"data", false)
        .expect("ピア開始 bidi への送信は成功するはず");
    let out = session.poll_output().expect("WT_STREAM の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtStream { .. } => {}
        other => panic!("WtStream を期待したが {other:?} だった"),
    }

    session
        .reset_stream(peer_id, 0)
        .expect("ピア開始 bidi へのリセットは成功するはず");
    let out = session.poll_output().expect("WT_RESET_STREAM の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtResetStream {
            stream_id,
            error_code,
            reliable_size,
        } => {
            assert_eq!(stream_id, peer_id);
            assert_eq!(error_code, 0);
            assert_eq!(
                reliable_size, 4,
                "送信済み 4 バイトが Reliable Size になること"
            );
        }
        other => panic!("WtResetStream を期待したが {other:?} だった"),
    }

    // WT_STOP_SENDING 受信では WT_RESET_STREAM が自動応答される
    let mut session2 = WtSession::server(WtConfig::default(), WtConfig::default());
    session2.initiate().expect("セッションを開始できるはず");
    let peer_id2 = wt_stream_id::first(true, true);
    open_peer_stream(&mut session2, peer_id2, b"hi");

    let mut encoder2 = CapsuleEncoder::new();
    encoder2.encode(&Capsule::WtStopSending {
        stream_id: peer_id2,
        error_code: 7,
    });
    session2
        .feed(&encoder2.take())
        .expect("feed に成功するはず");
    session2
        .process()
        .expect("ピア開始 bidi への WT_STOP_SENDING は受理されるはず");

    let out = session2
        .poll_output()
        .expect("WT_RESET_STREAM の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtResetStream {
            stream_id,
            error_code,
            reliable_size,
        } => {
            assert_eq!(stream_id, peer_id2);
            assert_eq!(error_code, 7);
            assert_eq!(reliable_size, 0, "未送信なので reliable_size は 0");
        }
        other => panic!("WtResetStream を期待したが {other:?} だった"),
    }
}

// ---- 送信専用ストリーム (ローカル開始 uni) への受信系操作の検証 (拒否と非回帰) ----

/// 送信専用ストリームへの stop_sending は stream_state_error になり、
/// ストリーム状態と出力が変化しないこと
/// (draft-ietf-webtrans-http2-15 Section 5.2 / RFC 9000 Section 19.5)
#[test]
fn stop_sending_send_only_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let stream_id = session.open_uni_stream().expect("ストリームを開けるはず");
    assert!(
        session.poll_output().is_none(),
        "ストリーム作成時点では出力が生成されないはず"
    );

    let err = session
        .stop_sending(stream_id, 7)
        .expect_err("送信専用ストリームへの stop_sending は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("send-only"));
    assert!(
        !session
            .stream(stream_id)
            .expect("ストリームが存在するはず")
            .stop_sending_sent(),
        "WT_STOP_SENDING 送信済みフラグが立ってはいけない"
    );
    assert!(
        session.poll_output().is_none(),
        "拒否時に出力が生成されてはいけない"
    );
}

/// 送信専用ストリームへの send_max_stream_data は stream_state_error になり、
/// 出力が生成されないこと
/// (draft-ietf-webtrans-http2-15 Section 5.2 / RFC 9000 Section 19.10)
#[test]
fn send_max_stream_data_send_only_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let stream_id = session.open_uni_stream().expect("ストリームを開けるはず");
    assert!(
        session.poll_output().is_none(),
        "ストリーム作成時点では出力が生成されないはず"
    );

    let err = session
        .send_max_stream_data(stream_id, 1_000_000)
        .expect_err("送信専用ストリームへの WT_MAX_STREAM_DATA 送信は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("send-only"));
    assert!(
        session.poll_output().is_none(),
        "拒否時に出力が生成されてはいけない"
    );
}

/// 送信専用ストリームへの grow_stream_recv_window は stream_state_error になり、
/// 受信ウィンドウと出力が変化しないこと
/// (draft-ietf-webtrans-http2-15 Section 5.2 / RFC 9000 Section 19.10)
#[test]
fn grow_stream_recv_window_send_only_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let stream_id = session.open_uni_stream().expect("ストリームを開けるはず");
    assert!(
        session.poll_output().is_none(),
        "ストリーム作成時点では出力が生成されないはず"
    );
    let before = session
        .stream(stream_id)
        .expect("ストリームが存在するはず")
        .recv_available();

    let err = session
        .grow_stream_recv_window(stream_id, 4096)
        .expect_err("送信専用ストリームの受信ウィンドウ拡張は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError
    );
    assert!(err.reason().contains("send-only"));
    assert_eq!(
        session
            .stream(stream_id)
            .expect("ストリームが存在するはず")
            .recv_available(),
        before,
        "拒否時に受信ウィンドウが変化してはいけない"
    );
    assert!(
        session.poll_output().is_none(),
        "拒否時に出力が生成されてはいけない"
    );

    // 拒否後に成功操作を行い、拒否時に capsule がエンコーダー内部へ
    // 積まれていないことを確認する (残留があれば先頭に現れる)
    session
        .send_stream_data(stream_id, b"data", false)
        .expect("拒否後の送信は成功するはず");
    let out = session.poll_output().expect("WT_STREAM の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtStream { .. } => {}
        other => panic!("WtStream を期待したが {other:?} だった"),
    }
}

/// 双方向ストリームでは stop_sending / send_max_stream_data /
/// grow_stream_recv_window が従来どおり受理されること。
///
/// `has_recv_part()` は双方向で真になるため、双方向クラスの受理確認は
/// ローカル開始 bidi で代表させる (ピア開始 bidi も同じ判定になる)。
#[test]
fn bidi_stream_recv_operations_accepted() {
    // stop_sending
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");
    let stream_id = session.open_bidi_stream().expect("ストリームを開けるはず");
    session
        .stop_sending(stream_id, 7)
        .expect("双方向ストリームへの stop_sending は成功するはず");
    let out = session.poll_output().expect("WT_STOP_SENDING の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtStopSending {
            stream_id: id,
            error_code,
        } => {
            assert_eq!(id, stream_id);
            assert_eq!(error_code, 7);
        }
        other => panic!("WtStopSending を期待したが {other:?} だった"),
    }

    // send_max_stream_data / grow_stream_recv_window
    // WT_STOP_SENDING 送信後は WT_MAX_STREAM_DATA を送れないため別セッションで検証する
    let mut session2 = WtSession::server(WtConfig::default(), WtConfig::default());
    session2.initiate().expect("セッションを開始できるはず");
    let stream_id2 = session2.open_bidi_stream().expect("ストリームを開けるはず");
    let recv_before = session2
        .stream(stream_id2)
        .expect("ストリームが存在するはず")
        .recv_available();

    // 単調増加になるよう、grow_stream_recv_window が送る recv_max + 4096 より
    // 大きい上限を後で送る
    session2
        .grow_stream_recv_window(stream_id2, 4096)
        .expect("双方向ストリームの受信ウィンドウ拡張は成功するはず");
    let out = session2
        .poll_output()
        .expect("WT_MAX_STREAM_DATA の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtMaxStreamData { stream_id, maximum } => {
            assert_eq!(stream_id, stream_id2);
            assert_eq!(maximum, recv_before + 4096);
        }
        other => panic!("WtMaxStreamData を期待したが {other:?} だった"),
    }
    assert_eq!(
        session2
            .stream(stream_id2)
            .expect("ストリームが存在するはず")
            .recv_available(),
        recv_before + 4096,
        "受信ウィンドウが拡張されること"
    );

    session2
        .send_max_stream_data(stream_id2, 1_000_000)
        .expect("双方向ストリームへの WT_MAX_STREAM_DATA 送信は成功するはず");
    let out = session2
        .poll_output()
        .expect("WT_MAX_STREAM_DATA の出力が必要");
    match decode_single_capsule(&out) {
        Capsule::WtMaxStreamData { stream_id, maximum } => {
            assert_eq!(stream_id, stream_id2);
            assert_eq!(maximum, 1_000_000);
        }
        other => panic!("WtMaxStreamData を期待したが {other:?} だった"),
    }
}

/// ストリームレベルのフロー制御違反時に output_buffer が汚染されないことを確認する
/// (draft-ietf-webtrans-http2-15 Section 6.6: ストリームレベルのフロー制御)
#[test]
fn send_stream_data_stream_flow_control_violation_does_not_pollute_buffer() {
    // ピアのストリームレベル送信上限を 5 バイトに制限
    let local_config = WtConfig::default();
    let peer_config = WtConfig {
        initial_max_stream_data_bidi_remote: 5,
        ..WtConfig::default()
    };
    let mut session = WtSession::client(local_config, peer_config);
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("open should succeed");

    // 上限を超える 10 バイトの送信はストリームレベルのフロー制御違反
    let err = session
        .send_stream_data(stream_id, b"0123456789", false)
        .unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
    assert!(err.reason().contains("stream send limit exceeded"));

    // 違反時に output_buffer にデータが残っていないこと
    assert!(
        session.poll_output().is_none(),
        "output_buffer must be empty after flow control violation"
    );
}

/// セッションレベルのフロー制御違反時に output_buffer が汚染されないことを確認する
/// (draft-ietf-webtrans-http2-15 Section 6.5: セッションレベルのフロー制御)
#[test]
fn send_stream_data_session_flow_control_violation_does_not_pollute_buffer() {
    // セッションレベルの送信上限を 5 バイトに制限 (ストリームレベルは十分大きく)
    let config = WtConfig {
        initial_max_data: 5,
        initial_max_stream_data_bidi_local: 1024,
        ..WtConfig::default()
    };
    let mut session = WtSession::client(config.clone(), config);
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("open should succeed");

    // ストリームレベルは通過するが、セッションレベルの上限を超える 10 バイトの送信
    let err = session
        .send_stream_data(stream_id, b"0123456789", false)
        .unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
    assert!(err.reason().contains("send window exhausted"));

    // 違反時に output_buffer にデータが残っていないこと
    assert!(
        session.poll_output().is_none(),
        "output_buffer must be empty after flow control violation"
    );
}

/// 送信側が常に send_offset と一致する Reliable Size を送ることを確認する
#[test]
fn wt_reset_stream_send_uses_send_offset() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // bidi ストリームを開いて 10 バイト送信
    let stream_id = session.open_bidi_stream().expect("open should succeed");
    session
        .send_stream_data(stream_id, b"0123456789", false)
        .expect("send should succeed");

    // WT_STREAM の出力を消費する
    let _ = session.poll_output().expect("output expected");

    // WT_RESET_STREAM を送信
    session
        .reset_stream(stream_id, 42)
        .expect("reset should succeed");

    let out = session.poll_output().expect("output expected");
    let capsule = decode_single_capsule(&out);
    match capsule {
        Capsule::WtResetStream {
            reliable_size,
            error_code,
            ..
        } => {
            // send_offset == 10 と一致する
            assert_eq!(reliable_size, 10, "reliable_size must equal send_offset");
            assert_eq!(error_code, 42);
        }
        other => panic!("expected WtResetStream, got {other:?}"),
    }
}

/// 非対称なフロー制御値: 送信制限がピアの広告値に従うことを確認する
/// (draft-ietf-webtrans-http2-15 Section 4.3.1: send_max はピアの SETTINGS 初期値)
#[test]
fn asymmetric_flow_control_send_uses_peer_value() {
    // ローカルは大きな受信上限を広告、ピアは小さな送信上限 (10 バイト) を広告
    let local_config = WtConfig {
        initial_max_data: 1_048_576,
        initial_max_stream_data_bidi_local: 1_048_576,
        ..WtConfig::default()
    };
    let peer_config = WtConfig {
        initial_max_data: 10,
        initial_max_stream_data_bidi_remote: 100,
        ..WtConfig::default()
    };
    let mut session = WtSession::client(local_config, peer_config);
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("open should succeed");

    // ピアの広告値 (10 バイト) 以内は送信可能
    session
        .send_stream_data(stream_id, b"0123456789", false)
        .expect("send within peer limit should succeed");

    // ピアの広告値を超えるとセッションレベルのフロー制御違反
    let err = session
        .send_stream_data(stream_id, b"x", false)
        .unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
    assert!(err.reason().contains("send window exhausted"));
}

/// 非対称なフロー制御値: ストリームレベルの送信制限がピアの広告値に従うことを確認する
/// (draft-ietf-webtrans-http2-15 Section 11.2: BIDI_REMOTE はピア視点で remote = ローカル開始)
#[test]
fn asymmetric_flow_control_stream_send_uses_peer_value() {
    // ローカルは大きなストリーム受信上限を広告、ピアは小さなストリーム送信上限 (5 バイト) を広告
    let local_config = WtConfig {
        initial_max_stream_data_bidi_local: 1_048_576,
        ..WtConfig::default()
    };
    let peer_config = WtConfig {
        initial_max_stream_data_bidi_remote: 5,
        ..WtConfig::default()
    };
    let mut session = WtSession::client(local_config, peer_config);
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("open should succeed");

    // ピアのストリームレベル広告値 (5 バイト) を超えるとフロー制御違反
    let err = session
        .send_stream_data(stream_id, b"0123456789", false)
        .unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
    assert!(err.reason().contains("stream send limit exceeded"));
}

/// 双方向ストリームが FIN 送受信で完全に閉じた後に HashMap から削除されることを確認する
#[test]
fn bidi_stream_removed_after_both_sides_close() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_bidi_stream().expect("open should succeed");

    // FIN 付きで送信 → 送信側は即座に DataRecvd (終端)
    session
        .send_stream_data(stream_id, b"hello", true)
        .expect("send should succeed");

    // まだ受信側が閉じていないのでストリームは残っている
    assert!(
        session.stream(stream_id).is_some(),
        "stream should still exist before recv side closes"
    );

    // ピアから FIN 付きデータを受信
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id,
        data: b"world".to_vec(),
        fin: true,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // poll_event で StreamData { fin: true } を pop すると DataRead に遷移して削除される
    let mut got_fin = false;
    while let Some(event) = session.poll_event() {
        if let WtEvent::StreamData { fin: true, .. } = event {
            got_fin = true;
        }
    }
    assert!(got_fin, "should receive StreamData with fin=true");

    // 両側が閉じたのでストリームは削除されている
    assert!(
        session.stream(stream_id).is_none(),
        "stream should be removed after both sides close"
    );
}

/// 送信専用単方向ストリームが FIN 送信で即座に削除されることを確認する
#[test]
fn uni_send_stream_removed_after_fin() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    let stream_id = session.open_uni_stream().expect("open should succeed");

    // FIN 付きで送信 → 送信専用 uni は即座に閉じて削除される
    session
        .send_stream_data(stream_id, b"data", true)
        .expect("send should succeed");

    assert!(
        session.stream(stream_id).is_none(),
        "send-only uni stream should be removed after FIN"
    );
}

/// クライアントロールのローカル開始 uni ストリーム (送信専用) への
/// ピア WT_STREAM が `stream_state_error` になり、`StreamData` が生成されない
/// ことを確認する (draft-ietf-webtrans-http2-15 Section 6.4 / RFC 9000 Section 2.1 / Section 19.8)。
#[test]
fn wt_stream_on_client_local_uni_stream_errors() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");

    // ローカル開始 uni ストリームを開く (FIN は送らず streams に残す)
    let stream_id = session
        .open_uni_stream()
        .expect("uni ストリームを開けるはず");

    // ピアがローカル開始 uni ストリーム ID へ WT_STREAM を送る
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id,
        data: b"from-peer".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    let err = session
        .process()
        .expect_err("ローカル開始 uni への WT_STREAM は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError,
        "予期しないエラー種別: {err}"
    );

    // 拒否されたので StreamData は生成されない
    while let Some(event) = session.poll_event() {
        assert!(
            !matches!(event, WtEvent::StreamData { .. }),
            "ローカル開始 uni への WT_STREAM で StreamData が生成されてはならない"
        );
    }

    // 拒否時に受信状態が変化していないこと
    let stream = session
        .stream(stream_id)
        .expect("拒否後もストリームは残るはず");
    assert_eq!(stream.recv_offset(), 0, "recv_offset は変化しないはず");
    assert!(!stream.has_received_data(), "受信記録は変化しないはず");
}

/// サーバーロールでもローカル開始 uni ストリーム (送信専用) への
/// ピア WT_STREAM が `stream_state_error` になり、`StreamData` が生成されないことを
/// 確認する (draft-ietf-webtrans-http2-15 Section 6.4 / RFC 9000 Section 2.1 / Section 19.8)。
#[test]
fn wt_stream_on_server_local_uni_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");

    // サーバー開始 uni ストリームを開く (FIN は送らず streams に残す)
    let stream_id = session
        .open_uni_stream()
        .expect("uni ストリームを開けるはず");

    // ピア (クライアント) がサーバー開始 uni ストリーム ID へ WT_STREAM を送る
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id,
        data: b"from-peer".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    let err = session
        .process()
        .expect_err("ローカル開始 uni への WT_STREAM は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError,
        "予期しないエラー種別: {err}"
    );

    // 拒否されたので StreamData は生成されない
    while let Some(event) = session.poll_event() {
        assert!(
            !matches!(event, WtEvent::StreamData { .. }),
            "ローカル開始 uni への WT_STREAM で StreamData が生成されてはならない"
        );
    }

    // 拒否時に受信状態が変化していないこと
    let stream = session
        .stream(stream_id)
        .expect("拒否後もストリームは残るはず");
    assert_eq!(stream.recv_offset(), 0, "recv_offset は変化しないはず");
    assert!(!stream.has_received_data(), "受信記録は変化しないはず");
}

/// ローカル開始 bidi ストリームへのピア WT_STREAM は受信可能であり、
/// 従来どおり `StreamData` を生成することを確認する
/// (draft-ietf-webtrans-http2-15 Section 6.4 / RFC 9000 Section 2.1)。
#[test]
fn wt_stream_on_local_bidi_stream_accepted() {
    let mut session = WtSession::client(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");

    let stream_id = session
        .open_bidi_stream()
        .expect("bidi ストリームを開けるはず");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id,
        data: b"from-peer".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    session
        .process()
        .expect("ローカル開始 bidi への WT_STREAM は受理されるはず");

    let mut got_data = false;
    while let Some(event) = session.poll_event() {
        if let WtEvent::StreamData {
            stream_id: id,
            data,
            fin,
        } = event
        {
            assert_eq!(id, stream_id, "受信したストリーム ID が一致するはず");
            assert_eq!(data, b"from-peer", "受信データが一致するはず");
            assert!(!fin, "FIN は設定されていないはず");
            got_data = true;
        }
    }
    assert!(
        got_data,
        "ローカル開始 bidi への WT_STREAM は StreamData を生成するはず"
    );
}

/// 受信専用単方向ストリームが FIN 受信 + poll_event で削除されることを確認する
#[test]
fn uni_recv_stream_removed_after_fin_and_poll() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("initiate should succeed");

    // クライアント開始 uni ストリーム (ID=2) をピアが開設
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: 2,
        data: b"data".to_vec(),
        fin: true,
    });
    session.feed(&encoder.take()).expect("feed should succeed");
    session.process().expect("process should succeed");

    // poll_event で StreamData { fin: true } を pop すると削除される
    while let Some(event) = session.poll_event() {
        if let WtEvent::StreamData { fin: true, .. } = event {
            break;
        }
    }

    assert!(
        session.stream(2).is_none(),
        "recv-only uni stream should be removed after FIN and poll"
    );
}

/// ストリーム削除後もフロー制御の累積カウントが正しく動作することを確認する
#[test]
fn flow_control_cumulative_count_works_after_stream_removal() {
    let peer_config = WtConfig {
        initial_max_streams_bidi: 2,
        ..WtConfig::default()
    };
    let mut session = WtSession::client(WtConfig::default(), peer_config);
    session.initiate().expect("initiate should succeed");

    // 2 つのストリームを開いて閉じる
    for _ in 0..2 {
        let stream_id = session.open_bidi_stream().expect("open should succeed");
        session
            .send_stream_data(stream_id, b"x", true)
            .expect("send should succeed");
    }

    // 累積カウントにより 3 つ目のストリームは制限に達する
    let err = session.open_bidi_stream().unwrap_err();
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::FlowControlError
    );
}

/// FIN でクローズ済みのピア開始 uni ストリームへの後続 WT_STREAM が
/// `stream_state_error` になることを確認する
/// (draft-ietf-webtrans-http2-15 Section 6.4 の MUST)。
#[test]
fn wt_stream_after_fin_closed_peer_uni_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");

    // ピア (クライアント) 開始 uni ストリーム (ID=2) に FIN 付きデータを送る
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: 2,
        data: b"data".to_vec(),
        fin: true,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    session.process().expect("process できるはず");

    // poll_event で FIN を読み取るとストリームが削除・記録される
    while let Some(event) = session.poll_event() {
        if let WtEvent::StreamData { fin: true, .. } = event {
            break;
        }
    }
    assert!(
        session.stream(2).is_none(),
        "FIN 後のストリームは削除されるはず"
    );

    // 同じ ID への後続 WT_STREAM は stream_state_error になる
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: 2,
        data: b"again".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    let err = session
        .process()
        .expect_err("クローズ済みストリームへの WT_STREAM は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError,
        "予期しないエラー種別: {err}"
    );

    // クローズ済みストリームの StreamOpened が再発行されないこと
    while let Some(event) = session.poll_event() {
        assert!(
            !matches!(event, WtEvent::StreamOpened { .. }),
            "クローズ済みストリームの StreamOpened が再発行されてはならない"
        );
    }
}

/// リセットでクローズ済みのピア開始 uni ストリームへの後続 WT_STREAM が
/// `stream_state_error` になることを確認する
/// (draft-ietf-webtrans-http2-15 Section 6.4 の MUST)。
#[test]
fn wt_stream_after_reset_closed_peer_uni_stream_errors() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");

    // ピア開始 uni ストリーム (ID=2) にデータを送る (FIN なし)
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: 2,
        data: b"data".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    session.process().expect("process できるはず");

    // WT_RESET_STREAM でリセットする (reliable_size は受信済みバイト数と一致必須)
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtResetStream {
        stream_id: 2,
        error_code: 0,
        reliable_size: 4,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    session.process().expect("process できるはず");
    assert!(
        session.stream(2).is_none(),
        "リセット後のストリームは削除されるはず"
    );

    // ここまでのイベント (初回 StreamOpened / StreamReset) をドレインする
    while session.poll_event().is_some() {}

    // 同じ ID への後続 WT_STREAM は stream_state_error になる
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: 2,
        data: b"again".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    let err = session
        .process()
        .expect_err("クローズ済みストリームへの WT_STREAM は拒否されるはず");
    assert_eq!(
        err.kind(),
        shiguredo_http2::webtransport::WtErrorKind::StreamStateError,
        "予期しないエラー種別: {err}"
    );

    // クローズ済みストリームの StreamOpened が再発行されないこと
    while let Some(event) = session.poll_event() {
        assert!(
            !matches!(event, WtEvent::StreamOpened { .. }),
            "クローズ済みストリームの StreamOpened が再発行されてはならない"
        );
    }
}

/// 記録に無い新規ピア開始ストリームへの WT_STREAM は従来どおり
/// `StreamOpened` を生成することを確認する。
#[test]
fn wt_stream_for_new_peer_stream_still_opens() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: 6,
        data: b"new".to_vec(),
        fin: false,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    session.process().expect("process できるはず");

    let mut got_opened = false;
    while let Some(event) = session.poll_event() {
        if let WtEvent::StreamOpened { stream_id: 6, .. } = event {
            got_opened = true;
        }
    }
    assert!(got_opened, "新規ストリームは StreamOpened を生成するはず");
}

/// Closed 状態では後続の capsule を無視し、新規ストリーム生成・イベント送出・
/// エラー化を行わないこと (後続 capsule の扱いは H2 draft に規定がなく実装判断。
/// WT_CLOSE_SESSION の受信で Closed へ遷移することは Section 6.12)。
#[test]
fn closed_session_ignores_subsequent_capsules() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::default());
    session.initiate().expect("セッションを開始できるはず");

    // 同一 DATA フレーム内で WT_CLOSE_SESSION → WT_STREAM → Datagram →
    // 未知ストリームへの WT_RESET_STREAM の順に届く
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtCloseSession {
        error_code: 0,
        reason: String::new(),
    });
    encoder.encode(&Capsule::WtStream {
        stream_id: 2,
        data: b"ignored".to_vec(),
        fin: false,
    });
    encoder.encode(&Capsule::Datagram {
        data: b"ignored".to_vec(),
    });
    encoder.encode(&Capsule::WtResetStream {
        stream_id: 999,
        error_code: 0,
        reliable_size: 0,
    });
    session.feed(&encoder.take()).expect("feed できるはず");
    session
        .process()
        .expect("Closed 後の capsule は無視され process は成功するはず");

    assert!(session.is_closed(), "WT_CLOSE_SESSION で Closed になるはず");
    assert!(
        session.stream(2).is_none(),
        "Closed 後の WT_STREAM で新規ストリームが生成されてはならない"
    );

    // SessionClosed が 1 回だけ送出され、他のイベントが送出されないこと
    let mut closed_count = 0;
    while let Some(event) = session.poll_event() {
        match event {
            WtEvent::SessionClosed { .. } => closed_count += 1,
            other => panic!("Closed 後に想定外のイベントが送出された: {other:?}"),
        }
    }
    assert_eq!(closed_count, 1, "SessionClosed は 1 回だけ送出されるはず");
}
