use shiguredo_http2::frame::StreamId;
use shiguredo_http2::stream_id::{
    ClientStreamId, NonZeroStreamId, Parity, ServerStreamId, StreamIdError,
};

#[test]
fn client_stream_id_new_ok() {
    let id = ClientStreamId::new(1).unwrap();
    assert_eq!(id.as_u32(), 1);
    let id = ClientStreamId::new(2147483647).unwrap();
    assert_eq!(id.as_u32(), 2147483647);
}

#[test]
fn client_stream_id_new_reserved() {
    assert_eq!(ClientStreamId::new(0), Err(StreamIdError::Reserved));
}

#[test]
fn client_stream_id_new_even() {
    assert_eq!(
        ClientStreamId::new(2),
        Err(StreamIdError::ParityMismatch {
            expected: Parity::Odd,
            got: 2,
        })
    );
}

#[test]
fn client_stream_id_from_static() {
    const ID: ClientStreamId = ClientStreamId::from_static(7);
    assert_eq!(ID.as_u32(), 7);
}

#[test]
fn server_stream_id_new_ok() {
    let id = ServerStreamId::new(2).unwrap();
    assert_eq!(id.as_u32(), 2);
}

#[test]
fn server_stream_id_new_odd() {
    assert_eq!(
        ServerStreamId::new(3),
        Err(StreamIdError::ParityMismatch {
            expected: Parity::Even,
            got: 3,
        })
    );
}

#[test]
fn server_stream_id_from_static() {
    const ID: ServerStreamId = ServerStreamId::from_static(4);
    assert_eq!(ID.as_u32(), 4);
}

#[test]
fn non_zero_stream_id_new_classifies_parity() {
    let id = NonZeroStreamId::new(1).unwrap();
    assert!(matches!(id, NonZeroStreamId::Client(_)));
    assert_eq!(id.as_u32(), 1);

    let id = NonZeroStreamId::new(4).unwrap();
    assert!(matches!(id, NonZeroStreamId::Server(_)));
    assert_eq!(id.as_u32(), 4);
}

#[test]
fn non_zero_stream_id_new_reserved() {
    assert_eq!(NonZeroStreamId::new(0), Err(StreamIdError::Reserved));
}

#[test]
fn non_zero_stream_id_from_static_client() {
    const ID: NonZeroStreamId = NonZeroStreamId::from_static(9);
    assert!(matches!(ID, NonZeroStreamId::Client(_)));
    assert_eq!(ID.as_u32(), 9);
}

#[test]
fn non_zero_stream_id_from_static_server() {
    const ID: NonZeroStreamId = NonZeroStreamId::from_static(8);
    assert!(matches!(ID, NonZeroStreamId::Server(_)));
    assert_eq!(ID.as_u32(), 8);
}

#[test]
fn non_zero_stream_id_client_and_server() {
    let id = NonZeroStreamId::Client(ClientStreamId::from_static(5));
    assert_eq!(id.client().unwrap().as_u32(), 5);
    assert!(id.server().is_none());

    let id = NonZeroStreamId::Server(ServerStreamId::from_static(6));
    assert!(id.client().is_none());
    assert_eq!(id.server().unwrap().as_u32(), 6);
}

#[test]
fn from_client_and_server_into_non_zero() {
    let c = ClientStreamId::from_static(3);
    let id: NonZeroStreamId = c.into();
    assert_eq!(id.as_u32(), 3);

    let s = ServerStreamId::from_static(4);
    let id: NonZeroStreamId = s.into();
    assert_eq!(id.as_u32(), 4);
}

#[test]
fn stream_id_from_wire_connection() {
    let id = StreamId::from_wire(0);
    assert_eq!(id, StreamId::Connection);
    assert_eq!(id.as_u32(), 0);
    assert!(id.non_zero().is_none());
}

#[test]
fn stream_id_from_wire_client() {
    let id = StreamId::from_wire(1);
    assert!(matches!(id, StreamId::Client(_)));
    assert_eq!(id.as_u32(), 1);
    assert!(id.non_zero().is_some());
}

#[test]
fn stream_id_from_wire_server() {
    let id = StreamId::from_wire(2);
    assert!(matches!(id, StreamId::Server(_)));
    assert_eq!(id.as_u32(), 2);
    assert!(id.non_zero().is_some());
}

#[test]
fn stream_id_display() {
    assert_eq!(StreamId::Connection.to_string(), "0");
    assert_eq!(StreamId::from_wire(7).to_string(), "7");
    assert_eq!(StreamId::from_wire(4).to_string(), "4");
}

#[test]
fn stream_id_from_conversions() {
    let c = ClientStreamId::from_static(3);
    let id: StreamId = c.into();
    assert_eq!(id.as_u32(), 3);

    let s = ServerStreamId::from_static(4);
    let id: StreamId = s.into();
    assert_eq!(id.as_u32(), 4);

    let nz = NonZeroStreamId::from_static(5);
    let id: StreamId = nz.into();
    assert_eq!(id.as_u32(), 5);
}

#[test]
fn stream_id_no_partial_ord() {
    // StreamId は PartialOrd を derive しない (variant をまたいだ順序比較は不適切)
    // as_u32() 経由で比較する
    let a = StreamId::from_wire(1);
    let b = StreamId::from_wire(3);
    assert!(a.as_u32() < b.as_u32());
}

#[test]
fn stream_id_error_display() {
    assert_eq!(
        StreamIdError::Reserved.to_string(),
        "stream ID 0 is reserved for connection control"
    );
    assert_eq!(
        StreamIdError::ParityMismatch {
            expected: Parity::Odd,
            got: 4,
        }
        .to_string(),
        "stream ID 4 parity mismatch: expected odd"
    );
    assert_eq!(
        StreamIdError::ParityMismatch {
            expected: Parity::Even,
            got: 5,
        }
        .to_string(),
        "stream ID 5 parity mismatch: expected even"
    );
    assert_eq!(
        StreamIdError::OutOfRange { value: u32::MAX }.to_string(),
        format!("stream ID {} exceeds 31-bit range", u32::MAX)
    );
}
