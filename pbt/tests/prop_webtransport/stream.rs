//! WebTransport ストリームフロー制御の PBT
//!
//! `WT_MAX_STREAM_DATA` の送信上限更新・広告・受信ウィンドウ拡張と、
//! ピア開始 bidi の暗黙生成経路の厳密増加規則を検証する。

use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, CapsuleEncoder, MAX_VALUE, WtConfig, WtErrorKind, WtEvent, WtSession,
    WtStream, WtStreamId, stream::stream_id as wt_stream_id,
};

use super::{CASES, SEED_ENV, sample_valid_varint_value};

/// 初期値に使う小さめの値を返すサンプラー
///
/// 更新値との比較で増加・非増加の両分岐を通すため、0 と 1 を含む小さな範囲から引く。
fn sample_small_window_value(ctx: &mut noprop::TestCaseContext) -> u64 {
    noprop::sample_with_boundaries(ctx, &[0u64, 1, 4096], noprop::Ratio::one_nth(5), |ctx| {
        noprop::sample_u64_in(ctx, 0..=4096)
    })
}

/// 更新値・増分に使うサンプラー
///
/// varint 上限 (2^62-1) に到達しない範囲を中心に、境界値 0 と 1 MiB を含める。
fn sample_stream_window_value(ctx: &mut noprop::TestCaseContext) -> u64 {
    noprop::sample_with_boundaries(ctx, &[0u64, 1, 1 << 20], noprop::Ratio::one_nth(5), |ctx| {
        noprop::sample_u64_in(ctx, 0..=(1 << 20))
    })
}

/// ピア開始の単方向ストリームを空の `WT_STREAM` で開く
///
/// 受信上限 0 でも新規ストリームの空 capsule は許可される。
fn open_peer_uni_stream(session: &mut WtSession) -> WtStreamId {
    let peer_id = wt_stream_id::first(true, false);
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::WtStream {
        stream_id: peer_id,
        data: Vec::new(),
        fin: false,
    });
    session
        .feed(&encoder.take())
        .expect("WT_STREAM の受信に成功しなければならない");
    session
        .process()
        .expect("WT_STREAM の処理に成功しなければならない");
    peer_id
}

/// `poll_output` から WT_MAX_STREAM_DATA を 1 つ取り出して値を検証する
///
/// 成功時に余剰の capsule が出力されていないことも確認する。
fn assert_single_wt_max_stream_data(
    session: &mut WtSession,
    stream_id: WtStreamId,
    expected_maximum: u64,
) {
    let out = session
        .poll_output()
        .expect("受理時は WT_MAX_STREAM_DATA を出力しなければならない");
    let mut decoder = CapsuleDecoder::new();
    decoder
        .feed(&out)
        .expect("受信データをデコーダーに渡せなければならない");
    match decoder
        .decode()
        .expect("capsule をデコードできなければならない")
    {
        Some(Capsule::WtMaxStreamData {
            stream_id: id,
            maximum,
        }) => {
            assert_eq!(id, stream_id);
            assert_eq!(
                maximum, expected_maximum,
                "出力された広告値が一致しなければならない"
            );
        }
        other => panic!("WT_MAX_STREAM_DATA を期待したが {other:?} だった"),
    }
    assert!(
        decoder
            .decode()
            .expect("余剰 capsule をデコードできなければならない")
            .is_none(),
        "受理時に余剰の capsule を出力してはいけない"
    );
}

/// `WtStream::update_send_max` が増加する値だけを受理することを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.6: そのストリームの初期値または最後に
/// 受理した上限を増加させない値は WT_FLOW_CONTROL_ERROR になる。
#[test]
fn prop_wt_max_stream_data_update_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial = sample_valid_varint_value(ctx);
        let mut stream = WtStream::new(0, initial, 0, true, true);
        let mut accepted = initial;

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            let value = sample_valid_varint_value(ctx);
            let result = stream.update_send_max(value);
            if value > accepted {
                result.expect("増加する上限は受理されなければならない");
                accepted = value;
            } else {
                let err = result.expect_err("増加しない上限は拒否されなければならない");
                assert_eq!(
                    err.kind(),
                    WtErrorKind::FlowControlError,
                    "拒否時はフロー制御エラーでなければならない"
                );
            }
            assert_eq!(
                stream.send_available(),
                accepted,
                "送信上限は最後に受理した値でなければならない"
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// `WtSession::send_max_stream_data` の広告が増加する値だけを出力し、
/// ストリームの受信上限と一致することを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.6: 初期値または最後に広告した上限を
/// 増加させない値は拒否し、受信上限と出力を変更しない。
#[test]
fn prop_wt_max_stream_data_advertisement_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial = sample_small_window_value(ctx);
        let config = WtConfig {
            initial_max_stream_data_uni: initial,
            ..WtConfig::default()
        };
        let mut session = WtSession::server(config, WtConfig::peer_default());
        session
            .initiate()
            .expect("セッションを開始できなければならない");
        let peer_id = open_peer_uni_stream(&mut session);

        let mut advertised = session
            .stream(peer_id)
            .expect("ストリームが存在しなければならない")
            .recv_max();

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            let value = sample_stream_window_value(ctx);
            let result = session.send_max_stream_data(peer_id, value);
            if value > advertised {
                result.expect("増加する広告は受理されなければならない");
                advertised = value;
                assert_eq!(
                    session
                        .stream(peer_id)
                        .expect("ストリームが存在しなければならない")
                        .recv_max(),
                    advertised,
                    "広告値と受信上限が一致しなければならない"
                );
                assert_single_wt_max_stream_data(&mut session, peer_id, advertised);
            } else {
                let err = result.expect_err("増加しない広告は拒否されなければならない");
                assert_eq!(
                    err.kind(),
                    WtErrorKind::FlowControlError,
                    "拒否時はフロー制御エラーでなければならない"
                );
                assert_eq!(
                    session
                        .stream(peer_id)
                        .expect("ストリームが存在しなければならない")
                        .recv_max(),
                    advertised,
                    "拒否時に受信上限が変わってはいけない"
                );
                assert!(
                    session.poll_output().is_none(),
                    "拒否時に WT_MAX_STREAM_DATA を出力してはいけない"
                );
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// `grow_stream_recv_window` の増分列が広告値と受信上限を正しく進めることを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.6: 増分 0 と varint 上限を超える拡張は拒否し、
/// 成功時は広告値と受信上限を一致させ、最後に出力した capsule の maximum と一致させる。
#[test]
fn prop_grow_stream_recv_window_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial = sample_small_window_value(ctx);
        let config = WtConfig {
            initial_max_stream_data_uni: initial,
            ..WtConfig::default()
        };
        let mut session = WtSession::server(config, WtConfig::peer_default());
        session
            .initiate()
            .expect("セッションを開始できなければならない");
        let peer_id = open_peer_uni_stream(&mut session);
        let mut advertised = session
            .stream(peer_id)
            .expect("ストリームが存在しなければならない")
            .recv_max();

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            // 0 / 小さい増分 / 中くらいの増分 / varint 上限近傍を混ぜて分岐を網羅する
            let increment = match noprop::sample_weighted_index(ctx, &[1, 1, 1, 1, 1]) {
                0 => 0,
                1 => noprop::sample_u64_in(ctx, 1..=(1 << 20)),
                2 => (1u64 << 20) + noprop::sample_u64_in(ctx, 0..=(1 << 20)),
                3 => MAX_VALUE - noprop::sample_u64_in(ctx, 0..=(1 << 20)),
                _ => u64::MAX,
            };
            let expected_ok = increment > 0
                && advertised
                    .checked_add(increment)
                    .is_some_and(|value| value <= MAX_VALUE);
            let result = session.grow_stream_recv_window(peer_id, increment);

            if expected_ok {
                result.expect("増加する拡張は受理されなければならない");
                advertised += increment;
                assert_eq!(
                    session
                        .stream(peer_id)
                        .expect("ストリームが存在しなければならない")
                        .recv_max(),
                    advertised,
                    "成功時に受信上限と広告値が一致しなければならない"
                );
                assert_single_wt_max_stream_data(&mut session, peer_id, advertised);
            } else {
                let err = result.expect_err("増加しない拡張は拒否されなければならない");
                assert_eq!(
                    err.kind(),
                    WtErrorKind::FlowControlError,
                    "拒否時はフロー制御エラーでなければならない"
                );
                assert_eq!(
                    session
                        .stream(peer_id)
                        .expect("ストリームが存在しなければならない")
                        .recv_max(),
                    advertised,
                    "拒否時に受信上限が変わってはいけない"
                );
                assert!(
                    session.poll_output().is_none(),
                    "拒否時に WT_MAX_STREAM_DATA を出力してはいけない"
                );
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// ピア開始 bidi の暗黙生成経路でも、増加する `WT_MAX_STREAM_DATA` だけが
/// ストリームと `StreamOpened` を生成することを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.6: 初期値を増加させない値は拒否し、
/// ストリームとイベントを残さない。
#[test]
fn prop_wt_max_stream_data_implicit_creation() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial = sample_small_window_value(ctx);
        let mut peer_config = WtConfig::peer_default();
        peer_config.initial_max_stream_data_bidi_local = initial;
        let peer_id = wt_stream_id::first(true, true);

        let mut session = WtSession::server(WtConfig::default(), peer_config);
        session
            .initiate()
            .expect("セッションを開始できなければならない");

        let maximum = sample_stream_window_value(ctx);
        let mut encoder = CapsuleEncoder::new();
        encoder.encode(&Capsule::WtMaxStreamData {
            stream_id: peer_id,
            maximum,
        });
        session
            .feed(&encoder.take())
            .expect("WT_MAX_STREAM_DATA の受信に成功しなければならない");
        let result = session.process();

        if maximum > initial {
            result.expect("初期値を増加させる値は受理されなければならない");
            assert_eq!(
                session
                    .stream(peer_id)
                    .expect("ストリームが生成されなければならない")
                    .send_max(),
                maximum,
                "受理した値が送信上限にならなければならない"
            );
            let mut opened = false;
            while let Some(event) = session.poll_event() {
                if let WtEvent::StreamOpened {
                    stream_id,
                    bidirectional,
                } = event
                    && stream_id == peer_id
                    && bidirectional
                {
                    opened = true;
                }
            }
            assert!(opened, "暗黙生成したストリームの StreamOpened が必要");

            // 生成済みストリームへの次の更新も同じ規則で検証される
            let next = sample_stream_window_value(ctx);
            let mut encoder = CapsuleEncoder::new();
            encoder.encode(&Capsule::WtMaxStreamData {
                stream_id: peer_id,
                maximum: next,
            });
            session
                .feed(&encoder.take())
                .expect("WT_MAX_STREAM_DATA の受信に成功しなければならない");
            let result = session.process();
            if next > maximum {
                result.expect("増加する更新は受理されなければならない");
                assert_eq!(
                    session
                        .stream(peer_id)
                        .expect("ストリームが存在しなければならない")
                        .send_max(),
                    next
                );
            } else {
                let err = result.expect_err("増加しない更新は拒否されなければならない");
                assert_eq!(err.kind(), WtErrorKind::FlowControlError);
                assert_eq!(
                    session
                        .stream(peer_id)
                        .expect("ストリームが存在しなければならない")
                        .send_max(),
                    maximum,
                    "拒否時に送信上限が変わってはいけない"
                );
            }
        } else {
            let err = result.expect_err("初期値を増加させない値は拒否されなければならない");
            assert_eq!(err.kind(), WtErrorKind::FlowControlError);
            assert!(
                session.stream(peer_id).is_none(),
                "拒否時にストリームを生成してはいけない"
            );
            assert!(
                session.poll_event().is_none(),
                "拒否時に StreamOpened を通知してはいけない"
            );
        }
        Ok(())
    })?;
    Ok(())
}
