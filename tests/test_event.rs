//! HTTP/2 イベントの単体テスト
//!
//! PBT のランダム生成では網羅が保証されないバリアント分類テスト。

use shiguredo_http2::frame::StreamId;
use shiguredo_http2::{ErrorCode, Event};

/// PriorityUpdateReceived は stream_id を持ち is_connection_level は false
#[test]
fn test_priority_update_is_stream_level() {
    let event = Event::PriorityUpdateReceived {
        stream_id: StreamId::from_wire(1),
        priority_field_value: vec![],
    };
    assert_eq!(event.stream_id(), Some(StreamId::from_wire(1)));
    assert!(!event.is_connection_level());
}

/// すべての接続レベルイベント種別をテスト
#[test]
fn test_all_connection_level_events() {
    let events = vec![
        Event::ConnectionPreface,
        Event::SettingsReceived { ack: false },
        Event::SettingsReceived { ack: true },
        Event::PingReceived {
            opaque_data: [0; 8],
            ack: false,
        },
        Event::GoawayReceived {
            last_stream_id: StreamId::Connection,
            error_code: ErrorCode::NoError,
            debug_data: vec![],
        },
        Event::WindowUpdateReceived {
            stream_id: StreamId::Connection,
            increment: 1000,
        },
        Event::ConnectionError {
            error_code: ErrorCode::ProtocolError,
            reason: "test".to_string(),
        },
    ];

    for event in events {
        assert!(
            event.is_connection_level(),
            "{:?} は接続レベルイベントであるべき",
            event
        );
        assert!(
            event.stream_id().is_none(),
            "{:?} は stream_id を持たないべき",
            event
        );
    }
}
