//! WebTransport フロー制御
//!
//! セッションレベルおよびストリームレベルのフロー制御を管理する。
//!
//! 注: 本モジュールは draft-ietf-webtrans-http2-15 に基づく実装であり、
//! draft の改訂や RFC 化に伴い仕様が変更される可能性がある。

use crate::webtransport::error::{WtError, WtResult};

/// Maximum Streams の上限 (2^60)
///
/// draft-ietf-webtrans-http2-15 Section 6.7 / Section 6.10:
/// "This value cannot exceed 2^60, as it is not possible to encode
/// stream IDs larger than 2^62-1"
const MAX_STREAMS_LIMIT: u64 = 1 << 60;

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
    /// ローカルが開いた双方向ストリーム数
    opened_streams_bidi: u64,
    /// ローカルが開いた単方向ストリーム数
    opened_streams_uni: u64,
}

impl WtFlowControl {
    /// 新しいフロー制御を生成する
    ///
    /// `send_max` にはピアが広告した送信上限、`recv_max` にはローカルが広告した受信上限を渡す。
    /// ストリーム数も local (ローカルが許可) / remote (ピアが許可) を別々に指定する。
    #[must_use]
    pub fn new(
        send_max: u64,
        recv_max: u64,
        max_streams_bidi_local: u64,
        max_streams_bidi_remote: u64,
        max_streams_uni_local: u64,
        max_streams_uni_remote: u64,
    ) -> Self {
        Self {
            send_max,
            send_offset: 0,
            recv_max,
            recv_offset: 0,
            max_streams_bidi_local,
            max_streams_bidi_remote,
            max_streams_uni_local,
            max_streams_uni_remote,
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

    /// 送信上限 (ピアが許可した最大バイト数) を取得する
    #[must_use]
    pub const fn send_max(&self) -> u64 {
        self.send_max
    }

    /// 受信上限 (ローカルが許可した最大バイト数) を取得する
    #[must_use]
    pub const fn recv_max(&self) -> u64 {
        self.recv_max
    }

    /// ローカルが許可した双方向ストリーム最大数を取得する
    #[must_use]
    pub const fn max_streams_bidi_local(&self) -> u64 {
        self.max_streams_bidi_local
    }

    /// ローカルが許可した単方向ストリーム最大数を取得する
    #[must_use]
    pub const fn max_streams_uni_local(&self) -> u64 {
        self.max_streams_uni_local
    }

    /// ピアが許可した双方向ストリーム最大数を取得する
    #[must_use]
    pub const fn max_streams_bidi_remote(&self) -> u64 {
        self.max_streams_bidi_remote
    }

    /// ピアが許可した単方向ストリーム最大数を取得する
    #[must_use]
    pub const fn max_streams_uni_remote(&self) -> u64 {
        self.max_streams_uni_remote
    }

    /// 送信を消費する
    pub fn consume_send(&mut self, size: u64) -> WtResult<()> {
        let new_offset = self.send_offset.saturating_add(size);
        // draft-ietf-webtrans-http2-15 Section 6.5: 送信総量は受信者が広告した値を超えてはならない (MUST NOT)
        if new_offset > self.send_max {
            return Err(WtError::flow_control_error("send window exhausted"));
        }
        self.send_offset = new_offset;
        Ok(())
    }

    /// 受信を消費する
    pub fn consume_recv(&mut self, size: u64) -> WtResult<()> {
        let new_offset = self.recv_offset.saturating_add(size);
        // draft-ietf-webtrans-http2-15 Section 6.5: 上限を超える受信は
        // WT_FLOW_CONTROL_ERROR のセッションエラー (MUST)
        if new_offset > self.recv_max {
            return Err(WtError::flow_control_error("recv window exceeded"));
        }
        self.recv_offset = new_offset;
        Ok(())
    }

    /// 送信上限を更新する (WT_MAX_DATA 受信時)
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.5:
    /// 値が減少した場合は WT_FLOW_CONTROL_ERROR セッションエラーを返す。
    ///
    /// 注: draft-ietf-webtrans-http2-15 由来の暫定仕様であり、RFC 化に伴い変更される可能性がある。
    pub fn update_send_max(&mut self, maximum: u64) -> WtResult<()> {
        if maximum < self.send_max {
            return Err(WtError::flow_control_error("WT_MAX_DATA value decreased"));
        }
        self.send_max = maximum;
        Ok(())
    }

    /// 受信上限を増加する (WT_MAX_DATA 送信前に呼び出す)
    ///
    /// varint の最大値 (2^62 - 1) を超えた場合はエラーを返す。
    pub fn add_recv_max(&mut self, increment: u64) -> WtResult<()> {
        let new_max = self.recv_max.saturating_add(increment);
        // varint エンコード可能な最大値を超えると WT_MAX_DATA のエンコードで panic するため
        if new_max > super::varint::MAX_VALUE {
            return Err(WtError::flow_control_error(
                "recv_max exceeds varint maximum value",
            ));
        }
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

    /// ピアからのストリームを受け入れ可能かどうかを返す
    ///
    /// RFC 9000 Section 4.6: stream_id < (max_streams * 4 + first_stream_id_of_type)
    /// のストリームのみ開設可能。順序外の stream ID は下位 ID も全て開いた扱いになる (RFC 9000 Section 2.1)。
    /// draft-ietf-webtrans-http2-15 Section 5.2 は QUIC のストリーム ID セマンティクスを継承する。
    ///
    /// 注: draft-ietf-webtrans-http2-15 由来の暫定仕様であり、RFC 化に伴い変更される可能性がある。
    #[must_use]
    pub fn can_accept_stream(&self, stream_id: u64) -> bool {
        let bidirectional = stream_id & 0x02 == 0;
        let first_id = stream_id & 0x03;
        let max_streams = if bidirectional {
            self.max_streams_bidi_local
        } else {
            self.max_streams_uni_local
        };
        // RFC 9000 Section 4.6: stream_id < max_streams * 4 + first_stream_id_of_type
        stream_id < max_streams.saturating_mul(4).saturating_add(first_id)
    }

    /// ストリーム数上限を更新する (WT_MAX_STREAMS 受信時)
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.7:
    /// 値が減少した場合は WT_FLOW_CONTROL_ERROR セッションエラーを返す。
    /// 2^60 を超える値も WT_FLOW_CONTROL_ERROR で MUST close。
    ///
    /// 注: draft-ietf-webtrans-http2-15 由来の暫定仕様であり、RFC 化に伴い変更される可能性がある。
    pub fn update_max_streams(&mut self, maximum: u64, bidirectional: bool) -> WtResult<()> {
        // draft-ietf-webtrans-http2-15 Section 6.7: 2^60 超過はセッションエラー
        if maximum > MAX_STREAMS_LIMIT {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_STREAMS value {} exceeds 2^60 limit",
                maximum
            )));
        }
        if bidirectional {
            if maximum < self.max_streams_bidi_remote {
                return Err(WtError::flow_control_error(
                    "WT_MAX_STREAMS (bidi) value decreased",
                ));
            }
            self.max_streams_bidi_remote = maximum;
        } else {
            if maximum < self.max_streams_uni_remote {
                return Err(WtError::flow_control_error(
                    "WT_MAX_STREAMS (uni) value decreased",
                ));
            }
            self.max_streams_uni_remote = maximum;
        }
        Ok(())
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
