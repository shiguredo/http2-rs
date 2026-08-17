//! ストリーム状態遷移の PBT (RFC 9113 Section 5.1)
//!
//! HTTP/2 ストリームの状態遷移を検証する。

use shiguredo_http2::stream::{StateMachine, StreamState};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// ストリーム操作
#[derive(Debug, Clone, Copy)]
enum StreamOp {
    SendHeaders { end_stream: bool },
    RecvHeaders { end_stream: bool },
    SendData { end_stream: bool },
    RecvData { end_stream: bool },
    SendRstStream,
    RecvRstStream,
}

/// ストリーム操作を 1 つ生成する
fn sample_stream_op(ctx: &mut noprop::TestCaseContext) -> StreamOp {
    match noprop::sample_weighted_index(ctx, &[2, 2, 2, 2, 1, 1]) {
        0 => StreamOp::SendHeaders {
            end_stream: noprop::sample_bool(ctx),
        },
        1 => StreamOp::RecvHeaders {
            end_stream: noprop::sample_bool(ctx),
        },
        2 => StreamOp::SendData {
            end_stream: noprop::sample_bool(ctx),
        },
        3 => StreamOp::RecvData {
            end_stream: noprop::sample_bool(ctx),
        },
        4 => StreamOp::SendRstStream,
        _ => StreamOp::RecvRstStream,
    }
}

/// 操作を適用する (エラーは無視して状態を返す)
///
/// `send_data` は validate のみで状態遷移しない設計のため、検査が通った場合に限り
/// 同時に `complete_send_data` を呼んで「DATA を実際に送信し終えた」モデルを表現する。
/// 実プロダクションでは `Connection::flush_stream_data` が最後の DATA を出力した時点で
/// `complete_send_data` を呼ぶ。
fn apply_op(sm: &mut StateMachine, op: StreamOp) -> Result<(), ()> {
    match op {
        StreamOp::SendHeaders { end_stream } => sm.send_headers(end_stream).map_err(|_| ()),
        StreamOp::RecvHeaders { end_stream } => sm.recv_headers(end_stream).map_err(|_| ()),
        StreamOp::SendData { end_stream } => {
            sm.send_data(end_stream).map_err(|_| ())?;
            sm.complete_send_data(end_stream).map_err(|_| ())
        }
        StreamOp::RecvData { end_stream } => sm.recv_data(end_stream).map_err(|_| ()),
        StreamOp::SendRstStream => {
            sm.send_rst_stream();
            Ok(())
        }
        StreamOp::RecvRstStream => {
            sm.recv_rst_stream();
            Ok(())
        }
    }
}

/// Closed 状態は吸収元 (absorbing state)
///
/// Closed 状態に到達した後、どの操作も状態を変えない。
/// 数学的意義: 吸収元の性質
#[test]
fn prop_closed_is_absorbing() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let before_count = noprop::sample_usize_in(ctx, 0..=9);
        let after_count = 1 + noprop::sample_usize_in(ctx, 0..=9);
        let ops_before: Vec<StreamOp> = (0..before_count).map(|_| sample_stream_op(ctx)).collect();
        let ops_after: Vec<StreamOp> = (0..after_count).map(|_| sample_stream_op(ctx)).collect();
        let mut sm = StateMachine::new();

        // 操作を適用して Closed 状態に到達させる
        for op in ops_before {
            let _ = apply_op(&mut sm, op);
        }

        // Closed 状態でない場合は RST_STREAM で強制的に Closed にする
        if sm.state() != StreamState::Closed {
            sm.send_rst_stream();
        }
        assert_eq!(sm.state(), StreamState::Closed);

        // Closed 状態になった後、どの操作を適用しても Closed のまま
        for op in ops_after {
            let _ = apply_op(&mut sm, op);
            assert_eq!(
                sm.state(),
                StreamState::Closed,
                "Closed state must be absorbing, but changed after {op:?}",
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// can_send/can_recv は状態と完全に対応する
///
/// 数学的意義: 状態と述語の対応関係
#[test]
fn prop_can_send_recv_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 操作列が空だと無検証で成立するため、ループ本体の実行をゲートする
    let executed_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let steps = noprop::sample_usize_in(ctx, 0..=19);
        let mut sm = StateMachine::new();
        let mut executed = false;

        for _ in 0..steps {
            executed = true;
            let _ = apply_op(&mut sm, sample_stream_op(ctx));

            let state = sm.state();

            // can_send は Open または HalfClosedRemote でのみ true
            let expected_can_send =
                matches!(state, StreamState::Open | StreamState::HalfClosedRemote);
            assert_eq!(
                state.can_send(),
                expected_can_send,
                "can_send mismatch for state {state:?}",
            );

            // can_recv は Open または HalfClosedLocal でのみ true
            let expected_can_recv =
                matches!(state, StreamState::Open | StreamState::HalfClosedLocal);
            assert_eq!(
                state.can_recv(),
                expected_can_recv,
                "can_recv mismatch for state {state:?}",
            );
        }
        if executed {
            executed_gate.set(executed_gate.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        executed_gate.get() > 0,
        "操作列が空で without check に成功した\n{runner}"
    );
    Ok(())
}

/// 有効な操作は成功し、無効な操作はエラーを返す (状態は不変)
///
/// 数学的意義: 状態遷移の閉包性
#[test]
fn prop_valid_transitions_only() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let executed_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let steps = noprop::sample_usize_in(ctx, 0..=19);
        let mut sm = StateMachine::new();
        let mut executed = false;

        for _ in 0..steps {
            executed = true;
            let op = sample_stream_op(ctx);
            let state_before = sm.state();
            let result = apply_op(&mut sm, op);
            let state_after = sm.state();

            match result {
                Ok(()) => {
                    // 成功した場合、状態が有効な遷移先であることを確認
                    // (Closed 以外の状態に戻ることはない)
                    if state_before == StreamState::Closed {
                        assert_eq!(state_after, StreamState::Closed);
                    }
                }
                Err(()) => {
                    // エラーの場合、RST_STREAM 以外では状態が変わらない
                    // (RST_STREAM は常に成功するので、ここには来ない)
                    assert_eq!(
                        state_before, state_after,
                        "State changed after failed operation {op:?}",
                    );
                }
            }
        }
        if executed {
            executed_gate.set(executed_gate.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        executed_gate.get() > 0,
        "操作列が空で無検証に成功した\n{runner}"
    );
    Ok(())
}

/// RST_STREAM はどの状態からも Closed に遷移する
///
/// 数学的意義: 終了状態への到達可能性
#[test]
fn prop_rst_stream_always_closes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let steps = noprop::sample_usize_in(ctx, 0..=9);
        let use_send = noprop::sample_bool(ctx);
        let mut sm = StateMachine::new();

        // 任意の状態に遷移
        for _ in 0..steps {
            let _ = apply_op(&mut sm, sample_stream_op(ctx));
        }

        let state_before = sm.state();

        // RST_STREAM を送信または受信
        if use_send {
            sm.send_rst_stream();
        } else {
            sm.recv_rst_stream();
        }

        // どの状態からも Closed に遷移する
        assert_eq!(
            sm.state(),
            StreamState::Closed,
            "RST_STREAM from {state_before:?} should result in Closed",
        );
        Ok(())
    })?;
    Ok(())
}

/// Idle から Open への遷移は HEADERS (end_stream=false) でのみ発生
///
/// 数学的意義: 初期状態からの有効な遷移
#[test]
fn prop_idle_to_open_via_headers() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let send = noprop::sample_bool(ctx);
        let mut sm = StateMachine::new();
        assert_eq!(sm.state(), StreamState::Idle);

        if send {
            sm.send_headers(false).expect("operation should succeed");
        } else {
            sm.recv_headers(false).expect("operation should succeed");
        }

        assert_eq!(sm.state(), StreamState::Open);
        Ok(())
    })?;
    Ok(())
}

/// HalfClosed 状態の対称性
///
/// send_headers(end_stream=true) -> HalfClosedLocal
/// recv_headers(end_stream=true) -> HalfClosedRemote
///
/// 数学的意義: 状態遷移の対称性
#[test]
fn prop_half_closed_symmetry() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let send = noprop::sample_bool(ctx);
        let mut sm = StateMachine::new();

        if send {
            sm.send_headers(true).expect("operation should succeed");
            assert_eq!(sm.state(), StreamState::HalfClosedLocal);
            // この状態では送信不可、受信可能
            assert!(!sm.state().can_send());
            assert!(sm.state().can_recv());
        } else {
            sm.recv_headers(true).expect("operation should succeed");
            assert_eq!(sm.state(), StreamState::HalfClosedRemote);
            // この状態では送信可能、受信不可
            assert!(sm.state().can_send());
            assert!(!sm.state().can_recv());
        }
        Ok(())
    })?;
    Ok(())
}

/// sent_end_stream と received_end_stream の整合性
///
/// 数学的意義: フラグと状態の整合性
#[test]
fn prop_end_stream_flags_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let executed_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let steps = noprop::sample_usize_in(ctx, 0..=19);
        let mut sm = StateMachine::new();
        let mut executed = false;

        for _ in 0..steps {
            executed = true;
            let _ = apply_op(&mut sm, sample_stream_op(ctx));

            let state = sm.state();

            // sent_end_stream は HalfClosedLocal または Closed でのみ true
            let expected_sent = matches!(state, StreamState::HalfClosedLocal | StreamState::Closed);
            assert_eq!(
                sm.sent_end_stream(),
                expected_sent,
                "sent_end_stream mismatch for state {state:?}",
            );

            // received_end_stream は HalfClosedRemote または Closed でのみ true
            let expected_received =
                matches!(state, StreamState::HalfClosedRemote | StreamState::Closed);
            assert_eq!(
                sm.received_end_stream(),
                expected_received,
                "received_end_stream mismatch for state {state:?}",
            );
        }
        if executed {
            executed_gate.set(executed_gate.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        executed_gate.get() > 0,
        "操作列が空で無検証に成功した\n{runner}"
    );
    Ok(())
}

/// Open 状態からの DATA 送信/受信
///
/// DATA (end_stream=true) で HalfClosed に遷移
/// DATA (end_stream=false) で Open のまま
///
/// 数学的意義: DATA フレームの状態遷移
#[test]
fn prop_open_data_transitions() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let end_stream = noprop::sample_bool(ctx);
        let send = noprop::sample_bool(ctx);
        let mut sm = StateMachine::new();

        // まず Open 状態にする
        sm.send_headers(false).expect("operation should succeed");
        assert_eq!(sm.state(), StreamState::Open);

        if send {
            sm.send_data(end_stream).expect("operation should succeed");
            // send_data は validate のみで状態遷移しない
            assert_eq!(sm.state(), StreamState::Open);
            sm.complete_send_data(end_stream).expect("should succeed");
            if end_stream {
                assert_eq!(sm.state(), StreamState::HalfClosedLocal);
            } else {
                assert_eq!(sm.state(), StreamState::Open);
            }
        } else {
            sm.recv_data(end_stream).expect("operation should succeed");
            if end_stream {
                assert_eq!(sm.state(), StreamState::HalfClosedRemote);
            } else {
                assert_eq!(sm.state(), StreamState::Open);
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// HalfClosed から Closed への遷移
///
/// HalfClosedLocal + recv_data(end_stream=true) -> Closed
/// HalfClosedRemote + send_data(end_stream=true) -> Closed
///
/// 数学的意義: 双方向クローズの必要性
#[test]
fn prop_half_closed_to_closed() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let local_first = noprop::sample_bool(ctx);
        let mut sm = StateMachine::new();

        // Open 状態にする
        sm.send_headers(false).expect("operation should succeed");

        if local_first {
            // ローカルが先に END_STREAM を送信 (validate + complete)
            sm.send_data(true).expect("operation should succeed");
            sm.complete_send_data(true)
                .expect("operation should succeed");
            assert_eq!(sm.state(), StreamState::HalfClosedLocal);

            // リモートから END_STREAM を受信
            sm.recv_data(true).expect("operation should succeed");
            assert_eq!(sm.state(), StreamState::Closed);
        } else {
            // リモートが先に END_STREAM を送信
            sm.recv_data(true).expect("operation should succeed");
            assert_eq!(sm.state(), StreamState::HalfClosedRemote);

            // ローカルが END_STREAM を送信 (validate + complete)
            sm.send_data(true).expect("operation should succeed");
            sm.complete_send_data(true)
                .expect("operation should succeed");
            assert_eq!(sm.state(), StreamState::Closed);
        }
        Ok(())
    })?;
    Ok(())
}
