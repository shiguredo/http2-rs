//! DATA フレーム関連の PBT
//!
//! DATA フレームのストリーム状態検証を含む。

use proptest::prelude::*;
use shiguredo_http2::{
    Connection, ErrorCode, Limits,
    frame::{DataFrame, Frame, SettingsFrame},
};

use super::{client_stream_id, encode_frame};

proptest! {
    /// idle ストリームへの DATA は PROTOCOL_ERROR
    ///
    /// RFC 9113 Section 5.1: idle ストリームへの DATA は PROTOCOL_ERROR
    #[test]
    fn prop_data_on_idle_stream_is_error(stream_id in client_stream_id()) {
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().unwrap();

        // SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).unwrap();
        server.process().unwrap();

        // idle ストリームに DATA を送信
        let data_frame = Frame::Data(DataFrame::new(stream_id, vec![1, 2, 3]));
        let data_bytes = encode_frame(&data_frame);
        server.feed(&data_bytes).unwrap();

        let result = server.process();
        prop_assert!(result.is_err());
        if let Err(e) = result {
            prop_assert!(e.is_connection_error());
            prop_assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
    }
}
