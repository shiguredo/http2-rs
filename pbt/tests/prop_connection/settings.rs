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
    /// クライアントがサーバーから ENABLE_PUSH=1 を受信した場合、PROTOCOL_ERROR
    #[test]
    fn prop_client_rejects_enable_push_from_server(_dummy in Just(())) {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // サーバーから ENABLE_PUSH=1 の SETTINGS を受信
        let mut settings = SettingsFrame::new();
        settings.add(Setting::EnablePush(true));
        let settings_bytes = encode_frame(&Frame::Settings(settings));
        client.feed(&settings_bytes).unwrap();

        let result = client.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// 最初のフレームが SETTINGS でない場合、PROTOCOL_ERROR
    ///
    /// RFC 9113 Section 3.4: 接続プリフェイス検証
    #[test]
    fn prop_first_frame_must_be_settings(_dummy in Just(())) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS ではなく PING を最初に送信
        let ping_frame = Frame::Ping(shiguredo_http2::frame::PingFrame::new([0u8; 8]));
        let ping_bytes = encode_frame(&ping_frame);
        server.feed(&ping_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }

    /// INITIAL_WINDOW_SIZE の無効値は FLOW_CONTROL_ERROR
    ///
    /// RFC 9113 Section 6.5.2: INITIAL_WINDOW_SIZE の無効な値は FLOW_CONTROL_ERROR
    #[test]
    fn prop_invalid_initial_window_size_is_flow_control_error(
        invalid_size in (MAX_INITIAL_WINDOW_SIZE + 1)..=u32::MAX,
    ) {
        let mut client = Connection::client(Limits::default());
        client.initiate().unwrap();

        // サーバーから無効な INITIAL_WINDOW_SIZE の SETTINGS を受信
        // decoder が Setting::from_wire で検証するため、raw バイト列を直接構築
        let mut settings_bytes = Vec::new();
        // フレームヘッダー: length=6, type=0x04 (SETTINGS), flags=0, stream_id=0
        settings_bytes.extend_from_slice(&[0x00, 0x00, 0x06, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
        // SETTINGS パラメータ: id=0x0004, value=invalid_size
        settings_bytes.extend_from_slice(&0x0004u16.to_be_bytes());
        settings_bytes.extend_from_slice(&invalid_size.to_be_bytes());
        client.feed(&settings_bytes).unwrap();

        let result = client.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::FlowControlError));
        }
    }

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
