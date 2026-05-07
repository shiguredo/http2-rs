//! ストリームバッファ
//!
//! HTTP/2 ストリームの送受信データを管理する。

use bytes::{Bytes, BytesMut};

/// 送信バッファ
///
/// 内部は `BytesMut` で連続領域として保持し、`pop` で `split_to(n).freeze()` により
/// `Bytes` として zero-copy に切り出す。relay (1:N 配信) で同じデータを複数宛先に
/// 渡す際に `Bytes::clone` (Arc inc) で済む。
#[derive(Debug, Default)]
pub struct SendBuffer {
    /// 送信待ちデータ
    data: BytesMut,
    /// 最大バッファサイズ
    max_size: usize,
}

impl SendBuffer {
    /// 新しい送信バッファを生成する
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            data: BytesMut::new(),
            max_size,
        }
    }

    /// バッファにデータを追加する
    ///
    /// バッファが満杯の場合は追加できなかったバイト数を返す。
    pub fn push(&mut self, data: &[u8]) -> usize {
        let available = self.max_size.saturating_sub(self.data.len());
        let to_push = data.len().min(available);
        self.data.extend_from_slice(&data[..to_push]);
        data.len() - to_push
    }

    /// バッファからデータを取り出す
    ///
    /// 指定したサイズまでのデータを `Bytes` として zero-copy に切り出す。
    pub fn pop(&mut self, max_size: usize) -> Bytes {
        let size = self.data.len().min(max_size);
        self.data.split_to(size).freeze()
    }

    /// バッファのデータ長を取得する
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// バッファが空かどうかを返す
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// バッファの残り容量を取得する
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.max_size.saturating_sub(self.data.len())
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

/// 受信バッファ
///
/// 内部は `BytesMut` で連続領域として保持し、`pop` / `take` で `Bytes` を
/// zero-copy に切り出して呼び出し側に渡す。
#[derive(Debug, Default)]
pub struct RecvBuffer {
    /// 受信データ
    data: BytesMut,
    /// 最大バッファサイズ
    max_size: usize,
}

impl RecvBuffer {
    /// 新しい受信バッファを生成する
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            data: BytesMut::new(),
            max_size,
        }
    }

    /// バッファにデータを追加する
    ///
    /// バッファが満杯の場合は `false` を返す。
    pub fn push(&mut self, data: &[u8]) -> bool {
        if self.data.len() + data.len() > self.max_size {
            return false;
        }
        self.data.extend_from_slice(data);
        true
    }

    /// バッファからデータを取り出す
    pub fn pop(&mut self, max_size: usize) -> Bytes {
        let size = self.data.len().min(max_size);
        self.data.split_to(size).freeze()
    }

    /// バッファの全データを取り出す
    pub fn take(&mut self) -> Bytes {
        self.data.split().freeze()
    }

    /// バッファのデータ長を取得する
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// バッファが空かどうかを返す
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// バッファの残り容量を取得する
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.max_size.saturating_sub(self.data.len())
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_buffer_push_pop() {
        let mut buf = SendBuffer::new(100);
        let remaining = buf.push(b"hello");
        assert_eq!(remaining, 0);
        assert_eq!(buf.len(), 5);

        let data = buf.pop(3);
        assert_eq!(&data[..], b"hel");
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
        assert_eq!(&data[..], b"hello");
        assert!(buf.is_empty());
    }

    #[test]
    fn test_recv_buffer_overflow() {
        let mut buf = RecvBuffer::new(10);
        assert!(buf.push(b"hello"));
        assert!(!buf.push(b"world!"));
        assert_eq!(buf.len(), 5);
    }
}
