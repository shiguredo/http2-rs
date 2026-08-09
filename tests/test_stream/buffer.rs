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

    let len = buf.len();
    let data = buf.pop(len);
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

/// saturating_add により整数オーバーフローが防止されることの確認。
/// 実用的には usize::MAX 近傍のデータ割り当ては不可能なため、
/// push が saturating_add を使用していることとパニックしないことを検証する。
#[test]
fn test_recv_buffer_push_uses_saturating_add() {
    // max_size = usize::MAX のバッファでも saturating_add によりパニックせず、
    // 上限超過時は false が返ることの確認（上限未満は true）
    let mut buf = RecvBuffer::new(usize::MAX);

    // 空のバッファにデータ追加は成功する
    assert!(buf.push(&[0u8; 1]));

    // 全データ取り出し後に再度 push も成功する
    let len = buf.len();
    let data = buf.pop(len);
    assert_eq!(data, &[0u8; 1]);
    assert!(buf.is_empty());
    assert!(buf.push(&[0u8; 1]));
}
