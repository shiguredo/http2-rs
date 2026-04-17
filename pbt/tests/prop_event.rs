//! HTTP/2 イベントの PBT
//!
//! Event 型のプロパティをテストする。

use proptest::prelude::*;
use shiguredo_http2::{ErrorCode, Event, HeaderField};

/// 有効なストリーム ID を生成する（1 以上）
fn valid_stream_id() -> impl Strategy<Value = u32> {
    1u32..=u32::MAX
}

/// ErrorCode を生成する Strategy
fn error_code_strategy() -> impl Strategy<Value = ErrorCode> {
    prop_oneof![
        Just(ErrorCode::NoError),
        Just(ErrorCode::ProtocolError),
        Just(ErrorCode::InternalError),
        Just(ErrorCode::FlowControlError),
        Just(ErrorCode::Cancel),
        any::<u32>().prop_map(ErrorCode::Unknown),
    ]
}

/// HeaderField を生成する Strategy
fn header_field_strategy() -> impl Strategy<Value = HeaderField> {
    ("[a-z-]{1,20}", "[a-zA-Z0-9]{1,50}")
        .prop_map(|(name, value)| HeaderField::from_str(&name, &value))
}

/// ストリームレベルの Event を生成する Strategy
fn stream_level_event() -> impl Strategy<Value = Event> {
    prop_oneof![
        // HeadersReceived
        (
            valid_stream_id(),
            prop::collection::vec(header_field_strategy(), 0..5),
            any::<bool>(),
            prop::option::of(prop::collection::vec(any::<u8>(), 1..20)),
        )
            .prop_map(|(stream_id, headers, end_stream, protocol)| {
                Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    protocol,
                }
            },),
        // DataReceived
        (
            valid_stream_id(),
            prop::collection::vec(any::<u8>(), 0..100),
            any::<bool>()
        )
            .prop_map(|(stream_id, data, end_stream)| {
                Event::DataReceived {
                    stream_id,
                    data,
                    end_stream,
                }
            }),
        // TrailersReceived
        (
            valid_stream_id(),
            prop::collection::vec(header_field_strategy(), 0..5)
        )
            .prop_map(|(stream_id, trailers)| {
                Event::TrailersReceived {
                    stream_id,
                    trailers,
                }
            }),
        // StreamReset
        (valid_stream_id(), error_code_strategy()).prop_map(|(stream_id, error_code)| {
            Event::StreamReset {
                stream_id,
                error_code,
            }
        }),
        // StreamClosed
        valid_stream_id().prop_map(|stream_id| { Event::StreamClosed { stream_id } }),
        // WindowUpdateReceived (stream_id != 0)
        (valid_stream_id(), 1u32..=u32::MAX).prop_map(|(stream_id, increment)| {
            Event::WindowUpdateReceived {
                stream_id,
                increment,
            }
        }),
        // PriorityUpdateReceived
        (valid_stream_id(), prop::collection::vec(any::<u8>(), 0..50)).prop_map(
            |(stream_id, priority_field_value)| {
                Event::PriorityUpdateReceived {
                    stream_id,
                    priority_field_value,
                }
            }
        ),
    ]
}

/// 接続レベルの Event を生成する Strategy
fn connection_level_event() -> impl Strategy<Value = Event> {
    prop_oneof![
        // ConnectionPreface
        Just(Event::ConnectionPreface),
        // SettingsReceived
        any::<bool>().prop_map(|ack| Event::SettingsReceived { ack }),
        // PingReceived
        (any::<[u8; 8]>(), any::<bool>())
            .prop_map(|(opaque_data, ack)| { Event::PingReceived { opaque_data, ack } }),
        // GoawayReceived
        (
            any::<u32>(),
            error_code_strategy(),
            prop::collection::vec(any::<u8>(), 0..50)
        )
            .prop_map(|(last_stream_id, error_code, debug_data)| {
                Event::GoawayReceived {
                    last_stream_id,
                    error_code,
                    debug_data,
                }
            }),
        // WindowUpdateReceived (stream_id == 0)
        (1u32..=u32::MAX).prop_map(|increment| {
            Event::WindowUpdateReceived {
                stream_id: 0,
                increment,
            }
        }),
        // ConnectionError
        (error_code_strategy(), "[a-zA-Z0-9 ]{0,50}")
            .prop_map(|(error_code, reason)| { Event::ConnectionError { error_code, reason } }),
    ]
}

/// すべての Event を生成する Strategy
fn any_event() -> impl Strategy<Value = Event> {
    prop_oneof![stream_level_event(), connection_level_event(),]
}

proptest! {
    /// ストリームレベルイベントは stream_id() が Some を返す
    #[test]
    fn prop_stream_level_event_has_stream_id(event in stream_level_event()) {
        prop_assert!(event.stream_id().is_some());
    }

    /// ストリームレベルイベントは is_connection_level() が false を返す
    #[test]
    fn prop_stream_level_event_not_connection_level(event in stream_level_event()) {
        prop_assert!(!event.is_connection_level());
    }

    /// 接続レベルイベントは is_connection_level() が true を返す
    #[test]
    fn prop_connection_level_event_is_connection_level(event in connection_level_event()) {
        prop_assert!(event.is_connection_level());
    }

    /// 接続レベルイベントは stream_id() が None を返す
    #[test]
    fn prop_connection_level_event_has_no_stream_id(event in connection_level_event()) {
        prop_assert!(event.stream_id().is_none());
    }

    /// WindowUpdateReceived の stream_id による分類
    ///
    /// stream_id == 0 なら接続レベル、それ以外はストリームレベル
    #[test]
    fn prop_window_update_classification(
        stream_id in any::<u32>(),
        increment in 1u32..=u32::MAX,
    ) {
        let event = Event::WindowUpdateReceived { stream_id, increment };

        if stream_id == 0 {
            prop_assert!(event.is_connection_level());
            prop_assert!(event.stream_id().is_none());
        } else {
            prop_assert!(!event.is_connection_level());
            prop_assert_eq!(event.stream_id(), Some(stream_id));
        }
    }

    /// stream_id() が返す値は実際のストリーム ID と一致する
    #[test]
    fn prop_stream_id_value_matches(
        stream_id in valid_stream_id(),
        data in prop::collection::vec(any::<u8>(), 0..10),
    ) {
        let events = vec![
            Event::HeadersReceived { stream_id, headers: vec![], end_stream: false, protocol: None },
            Event::DataReceived { stream_id, data: data.clone(), end_stream: false },
            Event::TrailersReceived { stream_id, trailers: vec![] },
            Event::StreamReset { stream_id, error_code: ErrorCode::NoError },
            Event::StreamClosed { stream_id },
            Event::WindowUpdateReceived { stream_id, increment: 1000 },
            Event::PriorityUpdateReceived { stream_id, priority_field_value: vec![] },
        ];

        for event in events {
            prop_assert_eq!(event.stream_id(), Some(stream_id));
        }
    }

    /// Event の Debug 実装が機能する
    #[test]
    fn prop_event_debug_not_panic(event in any_event()) {
        let debug_str = format!("{:?}", event);
        prop_assert!(!debug_str.is_empty());
    }

    /// Event の Clone が等価性を保持する
    #[test]
    fn prop_event_clone_equality(event in any_event()) {
        let cloned = event.clone();
        prop_assert_eq!(event, cloned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PriorityUpdateReceived は stream_id を持ち is_connection_level は false
    ///
    /// RFC 9218: PRIORITY_UPDATE はストリームに関連するフレームである
    #[test]
    fn test_priority_update_is_stream_level() {
        let event = Event::PriorityUpdateReceived {
            stream_id: 1,
            priority_field_value: vec![],
        };
        assert_eq!(event.stream_id(), Some(1));
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
                last_stream_id: 0,
                error_code: ErrorCode::NoError,
                debug_data: vec![],
            },
            Event::WindowUpdateReceived {
                stream_id: 0,
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
                "{:?} should be connection level",
                event
            );
            assert!(
                event.stream_id().is_none(),
                "{:?} should not have stream_id",
                event
            );
        }
    }
}
