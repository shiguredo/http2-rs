//! SETTINGS 関連の PBT
//!
//! SETTINGS フレームの検証、無効値チェック、変更禁止設定の検出を含む。

use proptest::prelude::*;
use shiguredo_http2::{
    Connection, ErrorCode, Limits, WindowSize,
    frame::{Frame, FrameDecoder, SettingsFrame, StreamId},
    settings::{DEFAULT_INITIAL_WINDOW_SIZE, MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, Setting},
};

use super::encode_frame;

proptest! {
    /// NO_RFC7540_PRIORITIES の変更は PROTOCOL_ERROR
    ///
    /// RFC 9218 Section 2.1: この設定は接続中に変更できない
    #[test]
    fn prop_no_rfc7540_priorities_change_is_error(
        initial_value in prop::bool::ANY,
    ) {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // サーバーから NO_RFC7540_PRIORITIES の SETTINGS を受信
        let mut settings1 = SettingsFrame::new();
        settings1.add(Setting::NoRfc7540Priorities(initial_value));
        let settings1_bytes = encode_frame(&Frame::Settings(settings1));
        client.feed(&settings1_bytes).unwrap();
        client.process().unwrap();

        // サーバーから異なる値の NO_RFC7540_PRIORITIES を受信
        let mut settings2 = SettingsFrame::new();
        settings2.add(Setting::NoRfc7540Priorities(!initial_value));
        let settings2_bytes = encode_frame(&Frame::Settings(settings2));
        client.feed(&settings2_bytes).unwrap();

        let result = client.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// 任意の SETTINGS シーケンスで true→false が出現した場合にのみ PROTOCOL_ERROR
    ///
    /// RFC 8441 §3 のプロパティを任意のシーケンスで検証する。
    /// クライアントロールで実行する（サーバーロールは別テストでカバー）。
    #[test]
    fn prop_enable_connect_protocol_sequence(
        sequence in prop::collection::vec(prop::bool::ANY, 1..=10),
    ) {
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate must succeed");

        // 最初にデフォルトの SETTINGS を送信して接続を Active にする
        let default_settings = SettingsFrame::new();
        let default_bytes = encode_frame(&Frame::Settings(default_settings));
        client.feed(&default_bytes).expect("feed must succeed");
        client.process().expect("process must succeed");

        // プロパティ: true→false が出現するかどうかを予測
        let mut saw_true = false;
        let mut expect_error = false;
        for &val in &sequence {
            if val {
                saw_true = true;
            } else if saw_true {
                expect_error = true;
                break;
            }
        }

        // シーケンスを 1 つずつ送信
        let mut actual_error = false;
        let mut actual_error_code = None;
        let mut actual_is_connection_error = false;
        for &val in &sequence {
            let mut sf = SettingsFrame::new();
            sf.add(Setting::EnableConnectProtocol(val));
            let bytes = encode_frame(&Frame::Settings(sf));
            client.feed(&bytes).expect("feed must succeed");
            if let Err(e) = client.process() {
                actual_error = true;
                actual_is_connection_error = e.is_connection_error();
                actual_error_code = e.error_code();
                break;
            }
        }

        prop_assert_eq!(
            actual_error, expect_error,
            "sequence={:?}, saw_true={}, expect_error={}, actual_error={}",
            sequence, saw_true, expect_error, actual_error
        );
        if actual_error {
            prop_assert!(
                actual_is_connection_error,
                "ダウングレードは接続エラーでなければならない"
            );
            prop_assert_eq!(
                actual_error_code,
                Some(ErrorCode::ProtocolError),
                "ダウングレードは PROTOCOL_ERROR でなければならない"
            );
        }
    }

    /// サーバーロールでも ENABLE_CONNECT_PROTOCOL ダウングレードを PROTOCOL_ERROR で拒否する
    ///
    /// RFC 8441 §3 はロール非依存。クライアントからの SETTINGS を受信するサーバー側でも
    /// 同じチェックが適用される。
    #[test]
    fn prop_enable_connect_protocol_sequence_server(
        sequence in prop::collection::vec(prop::bool::ANY, 1..=10),
    ) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().expect("initiate must succeed");

        // クライアントからの SETTINGS を受信して接続を Active にする
        let default_settings = SettingsFrame::new();
        let default_bytes = encode_frame(&Frame::Settings(default_settings));
        server.feed(&default_bytes).expect("feed must succeed");
        server.process().expect("process must succeed");

        let mut saw_true = false;
        let mut expect_error = false;
        for &val in &sequence {
            if val {
                saw_true = true;
            } else if saw_true {
                expect_error = true;
                break;
            }
        }

        let mut actual_error = false;
        let mut actual_error_code = None;
        let mut actual_is_connection_error = false;
        for &val in &sequence {
            let mut sf = SettingsFrame::new();
            sf.add(Setting::EnableConnectProtocol(val));
            let bytes = encode_frame(&Frame::Settings(sf));
            server.feed(&bytes).expect("feed must succeed");
            if let Err(e) = server.process() {
                actual_error = true;
                actual_is_connection_error = e.is_connection_error();
                actual_error_code = e.error_code();
                break;
            }
        }

        prop_assert_eq!(
            actual_error, expect_error,
            "sequence={:?}, saw_true={}, expect_error={}, actual_error={}",
            sequence, saw_true, expect_error, actual_error
        );
        if actual_error {
            prop_assert!(
                actual_is_connection_error,
                "ダウングレードは接続エラーでなければならない"
            );
            prop_assert_eq!(
                actual_error_code,
                Some(ErrorCode::ProtocolError),
                "ダウングレードは PROTOCOL_ERROR でなければならない"
            );
        }
    }

    /// EnableConnectProtocol を含まない SETTINGS が間に挟まっても追跡フラグは維持される
    ///
    /// RFC 8441 §3: ダウングレード検出は接続ライフタイム全体で追跡する
    #[test]
    fn prop_enable_connect_protocol_tracking_across_frames(
        gap_count in 1usize..=5,
        header_table_size in 1024u32..=65535,
    ) {
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate must succeed");

        // ENABLE_CONNECT_PROTOCOL=1 の SETTINGS を受信
        let mut settings1 = SettingsFrame::new();
        settings1.add(Setting::EnableConnectProtocol(true));
        let settings1_bytes = encode_frame(&Frame::Settings(settings1));
        client.feed(&settings1_bytes).expect("feed must succeed");
        client.process().expect("process must succeed");

        // EnableConnectProtocol を含まない SETTINGS を gap_count 回受信
        for _ in 0..gap_count {
            let mut gap_sf = SettingsFrame::new();
            gap_sf.add(Setting::HeaderTableSize(header_table_size));
            let gap_bytes = encode_frame(&Frame::Settings(gap_sf));
            client.feed(&gap_bytes).expect("feed must succeed");
            client.process().expect("process must succeed");
        }

        // ダウングレードを送信
        let mut settings3 = SettingsFrame::new();
        settings3.add(Setting::EnableConnectProtocol(false));
        let settings3_bytes = encode_frame(&Frame::Settings(settings3));
        client.feed(&settings3_bytes).expect("feed must succeed");

        let result = client.process();
        prop_assert!(result.is_err(), "フラグは間に挟まる SETTINGS でリセットされてはならない");
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// 同一 SETTINGS フレーム内の EnableConnectProtocol 重複は出現順に処理される
    ///
    /// RFC 9113 §6.5: 同一パラメータの重複は出現順で処理される。
    /// true→false の順なら PROTOCOL_ERROR、false→true の順なら成功。
    #[test]
    fn prop_enable_connect_protocol_intra_frame_duplicate(
        first in prop::bool::ANY,
    ) {
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate must succeed");

        let second = !first;
        let mut sf = SettingsFrame::new();
        sf.add(Setting::EnableConnectProtocol(first));
        sf.add(Setting::EnableConnectProtocol(second));
        let bytes = encode_frame(&Frame::Settings(sf));
        client.feed(&bytes).expect("feed must succeed");

        let result = client.process();

        // first=true, second=false → true→false でダウングレード → PROTOCOL_ERROR
        // first=false, second=true → false→true で有効化 → 成功
        if first {
            prop_assert!(result.is_err(), "true→false は PROTOCOL_ERROR でなければならない");
            if let Err(e) = result {
                prop_assert!(e.is_connection_error());
                prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
            }
        } else {
            prop_assert!(result.is_ok(), "false→true は成功しなければならない");
        }
    }

    /// `send_settings()` (preface 外部処理経路) でも同じ WINDOW_UPDATE が送信される
    #[test]
    fn prop_send_settings_emits_connection_window_update(
        size in (DEFAULT_INITIAL_WINDOW_SIZE + 1)..=MAX_INITIAL_WINDOW_SIZE,
    ) {
        let window = WindowSize::from_static(size);
        let limits = Limits::builder()
            .connection_window_size(window)
            .build()
            .expect("valid limits");
        let mut server = Connection::server(limits);
        server.mark_preface_received();
        server.send_settings().expect("send_settings");

        let output = server.poll_output().expect("output must contain settings");

        let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
        decoder.feed(&output);

        let mut found = None;
        while let Some(frame) = decoder.decode().expect("decode frame") {
            if let Frame::WindowUpdate(wu) = frame
                && matches!(wu.stream_id, StreamId::Connection)
            {
                found = Some(wu.window_size_increment.as_u32());
            }
        }
        let expected = size - DEFAULT_INITIAL_WINDOW_SIZE;
        prop_assert_eq!(
            found,
            Some(expected),
            "expected connection-level WINDOW_UPDATE with increment {}",
            expected
        );
    }
}
