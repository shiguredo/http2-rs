use shiguredo_http2::stream::{RecvBuffer, SendBuffer};

#[test]
fn test_send_buffer_push_pop() {
    let mut buf = SendBuffer::new(100);
    let remaining = buf.push(b"hello");
    assert_eq!(remaining, 0);
    assert_eq!(buf.len(), 5);

    let data = buf.pop(3);
    assert_eq!(data, b"hel");
    assert_eq!(buf.len(), 2);
}

#[test]
fn test_send_buffer_overflow() {
    let mut buf = SendBuffer::new(10);
    let remaining = buf.push(b"hello world!");
    assert_eq!(remaining, 2); // 12 - 10 = 2
    assert_eq!(buf.len(), 10);
}

#[test]
fn test_recv_buffer_push_pop() {
    let mut buf = RecvBuffer::new(100);
    assert!(buf.push(b"hello"));
    assert_eq!(buf.len(), 5);

    let data = buf.take();
    assert_eq!(data, b"hello");
    assert!(buf.is_empty());
}

#[test]
fn test_recv_buffer_overflow() {
    let mut buf = RecvBuffer::new(10);
    assert!(buf.push(b"hello"));
    assert!(!buf.push(b"world!"));
    assert_eq!(buf.len(), 5);
}
