use shiguredo_http2::SendError;

#[test]
fn display_connection_closed() {
    assert_eq!(
        SendError::ConnectionClosed.to_string(),
        "connection is closed"
    );
}

#[test]
fn display_goaway_sent() {
    assert_eq!(
        SendError::GoawaySent.to_string(),
        "GOAWAY has been sent; no new streams"
    );
}

#[test]
fn display_stream_not_open() {
    assert_eq!(
        SendError::StreamNotOpen { stream_id: 5 }.to_string(),
        "stream 5 is not open"
    );
}

#[test]
fn display_flow_control_exhausted() {
    assert_eq!(
        SendError::FlowControlExhausted.to_string(),
        "flow control window exhausted"
    );
}

#[test]
fn display_header_list_too_large() {
    let err = SendError::HeaderListTooLarge {
        actual: 16385,
        limit: 16384,
    };
    assert_eq!(
        err.to_string(),
        "header list size 16385 exceeds limit 16384"
    );
}
