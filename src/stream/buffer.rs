//! ストリームバッファ
//!
//! HTTP/2 ストリームの送受信データを管理する。

use std::collections::VecDeque;

/// 送信バッファ
#[derive(Debug, Default)]
pub struct SendBuffer {
    /// 送信待ちデータ
    data: VecDeque<u8>,
    /// 最大バッファサイズ
    max_size: usize,
}

impl SendBuffer {
    /// 新しい送信バッファを生成する
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            data: VecDeque::new(),
            max_size,
        }
    }

    /// バッファにデータを追加する
    ///
    /// バッファが満杯の場合は追加できなかったバイト数を返す。
    pub fn push(&mut self, data: &[u8]) -> usize {
        let available = self.max_size.saturating_sub(self.data.len());
        let to_push = data.len().min(available);
        self.data.extend(&data[..to_push]);
        data.len() - to_push
    }

    /// 指定したサイズのデータを追加できるかどうかを返す
    ///
    /// 入りきらない場合は `false` を返す。呼び出し側は `false` の場合に `push` を
    /// 呼ばないことで部分挿入を避けられる。
    #[must_use]
    pub(crate) fn can_push(&self, size: usize) -> bool {
        self.data.len().saturating_add(size) <= self.max_size
    }

    /// バッファからデータを取り出す
    ///
    /// 指定したサイズまでのデータを取り出す。
    pub fn pop(&mut self, max_size: usize) -> Vec<u8> {
        let size = self.data.len().min(max_size);
        self.data.drain(..size).collect()
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
}

/// 受信バッファ
#[derive(Debug, Default)]
pub struct RecvBuffer {
    /// 受信データ
    data: VecDeque<u8>,
    /// 最大バッファサイズ
    max_size: usize,
}

impl RecvBuffer {
    /// 新しい受信バッファを生成する
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            data: VecDeque::new(),
            max_size,
        }
    }

    /// バッファにデータを追加する
    ///
    /// バッファが満杯の場合は `false` を返す。
    pub fn push(&mut self, data: &[u8]) -> bool {
        if self.data.len().saturating_add(data.len()) > self.max_size {
            return false;
        }
        self.data.extend(data);
        true
    }

    /// バッファからデータを取り出す
    pub fn pop(&mut self, max_size: usize) -> Vec<u8> {
        let size = self.data.len().min(max_size);
        self.data.drain(..size).collect()
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
}
