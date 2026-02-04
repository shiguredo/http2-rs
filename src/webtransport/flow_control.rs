//! WebTransport フロー制御
//!
//! セッションレベルおよびストリームレベルのフロー制御を管理する。

use crate::webtransport::error::{WtError, WtResult};

/// WebTransport フロー制御
#[derive(Debug, Clone)]
pub struct WtFlowControl {
    // === セッションレベル ===
    /// ピアが許可した送信上限
    send_max: u64,
    /// 送信済みバイト数
    send_offset: u64,
    /// ローカルが許可した受信上限
    recv_max: u64,
    /// 受信済みバイト数
    recv_offset: u64,

    // === ストリーム数制限 ===
    /// ローカルが許可した双方向ストリーム最大数
    max_streams_bidi_local: u64,
    /// ピアが許可した双方向ストリーム最大数
    max_streams_bidi_remote: u64,
    /// ローカルが許可した単方向ストリーム最大数
    max_streams_uni_local: u64,
    /// ピアが許可した単方向ストリーム最大数
    max_streams_uni_remote: u64,
    /// 開いた双方向ストリーム数
    opened_streams_bidi: u64,
    /// 開いた単方向ストリーム数
    opened_streams_uni: u64,
}

impl WtFlowControl {
    /// 新しいフロー制御を生成する
    #[must_use]
    pub fn new(initial_max_data: u64, max_streams_bidi: u64, max_streams_uni: u64) -> Self {
        Self {
            send_max: initial_max_data,
            send_offset: 0,
            recv_max: initial_max_data,
            recv_offset: 0,
            max_streams_bidi_local: max_streams_bidi,
            max_streams_bidi_remote: max_streams_bidi,
            max_streams_uni_local: max_streams_uni,
            max_streams_uni_remote: max_streams_uni,
            opened_streams_bidi: 0,
            opened_streams_uni: 0,
        }
    }

    /// 送信可能な残りバイト数を取得する
    #[must_use]
    pub fn send_available(&self) -> u64 {
        self.send_max.saturating_sub(self.send_offset)
    }

    /// 受信可能な残りバイト数を取得する
    #[must_use]
    pub fn recv_available(&self) -> u64 {
        self.recv_max.saturating_sub(self.recv_offset)
    }

    /// 送信済みバイト数を取得する
    #[must_use]
    pub const fn send_offset(&self) -> u64 {
        self.send_offset
    }

    /// 受信済みバイト数を取得する
    #[must_use]
    pub const fn recv_offset(&self) -> u64 {
        self.recv_offset
    }

    /// 送信を消費する
    pub fn consume_send(&mut self, size: u64) -> WtResult<()> {
        let new_offset = self.send_offset.saturating_add(size);
        if new_offset > self.send_max {
            return Err(WtError::flow_control_error("send window exhausted"));
        }
        self.send_offset = new_offset;
        Ok(())
    }

    /// 受信を消費する
    pub fn consume_recv(&mut self, size: u64) -> WtResult<()> {
        let new_offset = self.recv_offset.saturating_add(size);
        if new_offset > self.recv_max {
            return Err(WtError::flow_control_error("recv window exceeded"));
        }
        self.recv_offset = new_offset;
        Ok(())
    }

    /// 送信上限を更新する (WT_MAX_DATA 受信時)
    pub fn update_send_max(&mut self, maximum: u64) {
        if maximum > self.send_max {
            self.send_max = maximum;
        }
    }

    /// 受信上限を増加する (WT_MAX_DATA 送信前に呼び出す)
    pub fn add_recv_max(&mut self, increment: u64) -> WtResult<()> {
        let new_max = self.recv_max.saturating_add(increment);
        self.recv_max = new_max;
        Ok(())
    }

    /// 双方向ストリームを開けるかどうかを返す
    #[must_use]
    pub fn can_open_bidi_stream(&self) -> bool {
        self.opened_streams_bidi < self.max_streams_bidi_remote
    }

    /// 単方向ストリームを開けるかどうかを返す
    #[must_use]
    pub fn can_open_uni_stream(&self) -> bool {
        self.opened_streams_uni < self.max_streams_uni_remote
    }

    /// ストリームを開いたことを記録する
    pub fn opened_stream(&mut self, bidirectional: bool) {
        if bidirectional {
            self.opened_streams_bidi += 1;
        } else {
            self.opened_streams_uni += 1;
        }
    }

    /// ストリーム数上限を更新する (WT_MAX_STREAMS 受信時)
    pub fn update_max_streams(&mut self, maximum: u64, bidirectional: bool) {
        if bidirectional {
            if maximum > self.max_streams_bidi_remote {
                self.max_streams_bidi_remote = maximum;
            }
        } else if maximum > self.max_streams_uni_remote {
            self.max_streams_uni_remote = maximum;
        }
    }

    /// ローカルのストリーム数上限を増加する (WT_MAX_STREAMS 送信前に呼び出す)
    pub fn add_max_streams_local(&mut self, increment: u64, bidirectional: bool) {
        if bidirectional {
            self.max_streams_bidi_local = self.max_streams_bidi_local.saturating_add(increment);
        } else {
            self.max_streams_uni_local = self.max_streams_uni_local.saturating_add(increment);
        }
    }

    /// 送信がブロックされているかどうかを返す
    #[must_use]
    pub fn is_send_blocked(&self) -> bool {
        self.send_offset >= self.send_max
    }

    /// 双方向ストリームがブロックされているかどうかを返す
    #[must_use]
    pub fn is_bidi_streams_blocked(&self) -> bool {
        self.opened_streams_bidi >= self.max_streams_bidi_remote
    }

    /// 単方向ストリームがブロックされているかどうかを返す
    #[must_use]
    pub fn is_uni_streams_blocked(&self) -> bool {
        self.opened_streams_uni >= self.max_streams_uni_remote
    }

    /// WINDOW_UPDATE を送信すべきかどうかを返す
    ///
    /// 受信ウィンドウが初期サイズの半分以下になったら更新を推奨
    #[must_use]
    pub fn should_send_max_data(&self, initial_max_data: u64) -> bool {
        self.recv_available() < initial_max_data / 2
    }

    /// 開いた双方向ストリーム数を取得する
    #[must_use]
    pub const fn opened_streams_bidi(&self) -> u64 {
        self.opened_streams_bidi
    }

    /// 開いた単方向ストリーム数を取得する
    #[must_use]
    pub const fn opened_streams_uni(&self) -> u64 {
        self.opened_streams_uni
    }
}

impl Default for WtFlowControl {
    fn default() -> Self {
        Self::new(1_048_576, 100, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_flow_control() {
        let fc = WtFlowControl::new(65536, 100, 50);
        assert_eq!(fc.send_available(), 65536);
        assert_eq!(fc.recv_available(), 65536);
    }

    #[test]
    fn test_consume_send() {
        let mut fc = WtFlowControl::new(65536, 100, 50);

        fc.consume_send(1000).unwrap();
        assert_eq!(fc.send_available(), 64536);
        assert_eq!(fc.send_offset(), 1000);
    }

    #[test]
    fn test_consume_send_exhausted() {
        let mut fc = WtFlowControl::new(100, 100, 50);

        fc.consume_send(100).unwrap();
        assert!(fc.consume_send(1).is_err());
    }

    #[test]
    fn test_consume_recv() {
        let mut fc = WtFlowControl::new(65536, 100, 50);

        fc.consume_recv(1000).unwrap();
        assert_eq!(fc.recv_available(), 64536);
        assert_eq!(fc.recv_offset(), 1000);
    }

    #[test]
    fn test_consume_recv_exceeded() {
        let mut fc = WtFlowControl::new(100, 100, 50);

        fc.consume_recv(100).unwrap();
        assert!(fc.consume_recv(1).is_err());
    }

    #[test]
    fn test_update_send_max() {
        let mut fc = WtFlowControl::new(65536, 100, 50);

        fc.consume_send(65536).unwrap();
        assert!(fc.is_send_blocked());

        fc.update_send_max(131072);
        assert!(!fc.is_send_blocked());
        assert_eq!(fc.send_available(), 65536);
    }

    #[test]
    fn test_stream_limits() {
        let mut fc = WtFlowControl::new(65536, 2, 1);

        assert!(fc.can_open_bidi_stream());
        assert!(fc.can_open_uni_stream());

        fc.opened_stream(true);
        fc.opened_stream(true);
        assert!(!fc.can_open_bidi_stream());
        assert!(fc.is_bidi_streams_blocked());

        fc.opened_stream(false);
        assert!(!fc.can_open_uni_stream());
        assert!(fc.is_uni_streams_blocked());
    }

    #[test]
    fn test_update_max_streams() {
        let mut fc = WtFlowControl::new(65536, 2, 1);

        fc.opened_stream(true);
        fc.opened_stream(true);
        assert!(!fc.can_open_bidi_stream());

        fc.update_max_streams(4, true);
        assert!(fc.can_open_bidi_stream());
    }

    #[test]
    fn test_should_send_max_data() {
        let mut fc = WtFlowControl::new(65536, 100, 50);

        assert!(!fc.should_send_max_data(65536));

        fc.consume_recv(40000).unwrap();
        assert!(fc.should_send_max_data(65536));
    }
}
