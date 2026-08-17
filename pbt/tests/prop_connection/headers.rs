//! HEADERS 関連の PBT
//!
//! Continuation フレームの状態遷移を含む。

use shiguredo_http2::{
    Connection, ErrorCode, Limits, NonZeroStreamId,
    frame::{ContinuationFrame, Frame, HeadersFrame, SettingsFrame},
};

use super::{encode_frame, sample_client_stream_id};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// HEADERS フレームを作成する（END_HEADERS なし）
fn create_headers_without_end_headers(
    stream_id: NonZeroStreamId,
    fragment: Vec<u8>,
) -> HeadersFrame {
    HeadersFrame::new(stream_id, fragment)
        .with_end_stream(false)
        .with_end_headers(false)
}

/// CONTINUATION フレームを作成する
fn create_continuation(
    stream_id: NonZeroStreamId,
    fragment: Vec<u8>,
    end_headers: bool,
) -> ContinuationFrame {
    ContinuationFrame::new(stream_id, fragment).with_end_headers(end_headers)
}

/// CONTINUATION フレームのストリーム ID が一致しない場合、PROTOCOL_ERROR
/// RFC 9113 Section 6.10: END_HEADERS 未設定の後に異なるストリームのフレームを受信した場合は PROTOCOL_ERROR の接続エラー (MUST)。
#[test]
fn prop_continuation_stream_id_mismatch_is_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let first_id = sample_client_stream_id(ctx);
        // 2 つ目は first_id と異なる値を valid-by-construction で引き直す
        // (許容率 0.99 のため 8 回の試行で十分)
        let second_id = noprop::sample_with_rejection(ctx, 8, |ctx| {
            let id = sample_client_stream_id(ctx);
            (id != first_id).then_some(id)
        });

        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().expect("initiate should succeed");

        // SETTINGS を受信
        let settings_frame = Frame::Settings(SettingsFrame::new());
        let settings_bytes = encode_frame(&settings_frame);
        server.feed(&settings_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        // HEADERS (END_HEADERS なし)
        let headers = create_headers_without_end_headers(first_id, vec![0x82]);
        let headers_bytes = encode_frame(&Frame::Headers(headers));
        server.feed(&headers_bytes).expect("feed should succeed");
        server.process().expect("process should succeed");

        // 異なるストリーム ID で CONTINUATION を送信
        let continuation = create_continuation(second_id, vec![0x84], true);
        let continuation_bytes = encode_frame(&Frame::Continuation(continuation));
        server
            .feed(&continuation_bytes)
            .expect("feed should succeed");

        let result = server.process();
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.is_connection_error());
            assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
        Ok(())
    })?;
    Ok(())
}
