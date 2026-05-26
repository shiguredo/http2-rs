//! フロー制御の PBT
//!
//! 本 PBT は `src/flow_control.rs` (接続/ストリームレベル) に対応する。
//! `src/webtransport/flow_control.rs` 用の PBT は将来 `pbt/tests/prop_webtransport/flow_control.rs` に配置する。

use proptest::prelude::*;
use shiguredo_http2::FlowControl;

/// 有効なウィンドウサイズを生成する
fn valid_window_size() -> impl Strategy<Value = u32> {
    1..=2_147_483_647u32
}

proptest! {
    /// フロー制御の初期化テスト
    #[test]
    fn prop_flow_control_init(initial_window in valid_window_size()) {
        let fc = FlowControl::new(initial_window);
        prop_assert_eq!(fc.send_window(), i64::from(initial_window));
        prop_assert_eq!(fc.recv_window(), i64::from(initial_window));
        prop_assert_eq!(fc.send_initial(), initial_window);
        prop_assert_eq!(fc.recv_initial(), initial_window);
    }

    /// 送信/受信ウィンドウ分離初期化テスト
    ///
    /// RFC 9113 Section 5.2: ストリームのフロー制御において、
    /// 送信ウィンドウはリモートの initial_window_size、
    /// 受信ウィンドウはローカルの initial_window_size で初期化する。
    #[test]
    fn prop_separate_windows_init(
        send_initial in valid_window_size(),
        recv_initial in valid_window_size(),
    ) {
        let fc = FlowControl::with_separate_windows(send_initial, recv_initial);
        prop_assert_eq!(fc.send_window(), i64::from(send_initial));
        prop_assert_eq!(fc.recv_window(), i64::from(recv_initial));
        prop_assert_eq!(fc.send_initial(), send_initial);
        prop_assert_eq!(fc.recv_initial(), recv_initial);
    }

    /// 送信ウィンドウ消費テスト
    #[test]
    fn prop_consume_send(
        initial_window in 100..=65535u32,
        consume_size in 0..=100usize,
    ) {
        let mut fc = FlowControl::new(initial_window);
        let result = fc.consume_send(consume_size);

        if consume_size <= initial_window as usize {
            prop_assert!(result.is_ok());
            prop_assert_eq!(fc.send_window(), i64::from(initial_window) - consume_size as i64);
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// 送信可能サイズの計算テスト
    #[test]
    fn prop_send_available(
        initial_window in 1..=65535u32,
        consume_size in 0..=65535usize,
    ) {
        let mut fc = FlowControl::new(initial_window);
        let consume = consume_size.min(initial_window as usize);
        fc.consume_send(consume).unwrap();

        let available = fc.send_available();
        let expected = (initial_window as usize).saturating_sub(consume);
        prop_assert_eq!(available, expected);
    }

    /// WINDOW_UPDATE 受信テスト
    #[test]
    fn prop_window_update(
        initial_window in 1..=1_000_000u32,
        consume_size in 0..=1_000_000usize,
        increment in 1..=1_000_000u32,
    ) {
        let mut fc = FlowControl::new(initial_window);
        let consume = consume_size.min(initial_window as usize);
        fc.consume_send(consume).unwrap();

        let before = fc.send_window();
        let result = fc.recv_window_update(increment);

        let new_window = before + i64::from(increment);
        if new_window > i64::from(shiguredo_http2::MAX_WINDOW_SIZE) {
            prop_assert!(result.is_err());
        } else {
            prop_assert!(result.is_ok());
            prop_assert_eq!(fc.send_window(), new_window);
        }
    }

    /// ウィンドウサイズ更新テスト
    #[test]
    fn prop_update_initial_window_size(
        initial_window in 1..=65535u32,
        consume_size in 0..=32767usize,
        new_initial in 1..=131070u32,
    ) {
        let mut fc = FlowControl::new(initial_window);
        let consume = consume_size.min(initial_window as usize);
        fc.consume_send(consume).unwrap();

        let before = fc.send_window();
        let result = fc.update_initial_window_size(new_initial);

        let delta = i64::from(new_initial) - i64::from(initial_window);
        let new_window = before + delta;

        if new_window > i64::from(shiguredo_http2::MAX_WINDOW_SIZE) {
            prop_assert!(result.is_err());
        } else {
            prop_assert!(result.is_ok());
            prop_assert_eq!(fc.send_window(), new_window);
            prop_assert_eq!(fc.send_initial(), new_initial);
        }
    }

    /// add_recv_window に increment == 0 を渡すとエラーになる (RFC 9113 Section 6.9)
    #[test]
    fn prop_add_recv_window_zero_rejected(
        initial_window in valid_window_size(),
    ) {
        let mut fc = FlowControl::new(initial_window);
        prop_assert!(fc.add_recv_window(0).is_err());
    }

    /// should_send_window_update / window_update_increment が send_initial に依存しないことを検証する
    ///
    /// 任意の (send_initial_a, send_initial_b, recv_initial, consume_amount) に対して、
    /// with_separate_windows(send_initial_a, recv_initial) と
    /// with_separate_windows(send_initial_b, recv_initial) で同じ量を consume_recv した後の
    /// 結果が一致することを検証する。
    #[test]
    fn prop_recv_methods_independent_of_send_initial(
        send_initial_a in valid_window_size(),
        send_initial_b in valid_window_size(),
        recv_initial in 1..=1_000_000u32,
        consume_amount in 0..=1_000_000usize,
    ) {
        let mut fc_a = FlowControl::with_separate_windows(send_initial_a, recv_initial);
        let mut fc_b = FlowControl::with_separate_windows(send_initial_b, recv_initial);

        let consume = consume_amount.min(recv_initial as usize);
        fc_a.consume_recv(consume).unwrap();
        fc_b.consume_recv(consume).unwrap();

        prop_assert_eq!(
            fc_a.should_send_window_update(),
            fc_b.should_send_window_update()
        );
        prop_assert_eq!(
            fc_a.window_update_increment(),
            fc_b.window_update_increment()
        );
    }

    /// update_initial_window_size が recv_initial を変更しないことを検証する
    #[test]
    fn prop_update_initial_preserves_recv_initial(
        send_initial in valid_window_size(),
        recv_initial in valid_window_size(),
        new_size in 1..=131070u32,
    ) {
        let mut fc = FlowControl::with_separate_windows(send_initial, recv_initial);
        let recv_initial_before = fc.recv_initial();
        let _ = fc.update_initial_window_size(new_size);
        prop_assert_eq!(fc.recv_initial(), recv_initial_before);
    }

    /// recv_window > recv_initial のケースで window_update_increment が 0 を返し、
    /// should_send_window_update が false を返すことを検証する
    #[test]
    fn prop_recv_window_above_initial(
        send_initial in valid_window_size(),
        recv_initial in 1..=1_000_000u32,
        extra in 1..=1_000_000u32,
    ) {
        let mut fc = FlowControl::with_separate_windows(send_initial, recv_initial);
        // recv_window を recv_initial 超に増加させる
        let new_recv = i64::from(recv_initial) + i64::from(extra);
        if new_recv <= i64::from(shiguredo_http2::MAX_WINDOW_SIZE) {
            fc.add_recv_window(extra).unwrap();
            prop_assert!(!fc.should_send_window_update());
            prop_assert_eq!(fc.window_update_increment(), 0);
        }
    }

    /// フロー制御の不変条件テスト
    /// - 消費量がウィンドウサイズを超えない
    /// - WINDOW_UPDATE でオーバーフローしない
    #[test]
    fn prop_flow_control_invariants(
        initial_window in 1..=65535u32,
        operations in prop::collection::vec(
            prop_oneof![
                (0..=1000usize).prop_map(Op::Consume),
                (1..=10000u32).prop_map(Op::WindowUpdate),
            ],
            0..20
        ),
    ) {
        let mut fc = FlowControl::new(initial_window);

        for op in operations {
            match op {
                Op::Consume(size) => {
                    let available = fc.send_available();
                    if size <= available {
                        fc.consume_send(size).unwrap();
                        // 不変条件: 送信ウィンドウは負にならない（消費後も正または 0）
                        prop_assert!(fc.send_window() >= 0);
                    }
                }
                Op::WindowUpdate(increment) => {
                    if fc.send_window() + i64::from(increment) <= i64::from(shiguredo_http2::MAX_WINDOW_SIZE) {
                        fc.recv_window_update(increment).unwrap();
                        // 不変条件: ウィンドウは MAX_WINDOW_SIZE を超えない
                        prop_assert!(fc.send_window() <= i64::from(shiguredo_http2::MAX_WINDOW_SIZE));
                    }
                }
            }
        }
    }
}

/// フロー制御操作
#[derive(Debug, Clone)]
enum Op {
    /// 送信データ消費
    Consume(usize),
    /// WINDOW_UPDATE 受信
    WindowUpdate(u32),
}
