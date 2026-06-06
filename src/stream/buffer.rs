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

    /// バッファの全データを取り出す
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.data).into()
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
