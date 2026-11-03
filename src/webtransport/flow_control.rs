//! WebTransport フロー制御
//!
//! セッションレベルおよびストリームレベルのフロー制御を管理する。
//!
//! 注: 本モジュールは draft-ietf-webtrans-http2-15 に基づく実装であり、
//! draft の改訂や RFC 化に伴い仕様が変更される可能性がある。
//! `WT_MAX_DATA` の厳密増加規則 (`update_send_max` / `update_recv_max`) だけは
//! draft-ietf-webtrans-http2-16 Section 6.5 に従う。

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
    /// ローカルの受信上限 (初期値は `WtConfig::initial_max_data`)
    ///
    /// `WtSession` 経由で更新した場合はピアへ最後に広告した値と一致する。
    /// `update_recv_max` を直接呼び出した場合は広告されないため一致しない。
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
    /// `recv_max` は varint 上限 (2^62-1) 以下でなければならない (呼び出し側の事前条件)。
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

    /// ローカルの受信上限 (初期値は `WtConfig::initial_max_data`) を取得する
    ///
    /// `WtSession` 経由で更新した場合はピアへ最後に広告した値と一致する。
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
    /// draft-ietf-webtrans-http2-16 Section 6.5:
    /// ピアの初期値または最後に受理した上限を増加させない値は
    /// WT_FLOW_CONTROL_ERROR セッションエラーを返す。
    /// varint で表現できない値も拒否する (RFC 9000 Section 16)。
    ///
    /// 注: draft-ietf-webtrans-http2-16 由来の暫定仕様であり、RFC 化に伴い変更される可能性がある。
    pub fn update_send_max(&mut self, maximum: u64) -> WtResult<()> {
        if maximum > super::varint::MAX_VALUE {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_DATA value {maximum} exceeds varint maximum {}",
                super::varint::MAX_VALUE
            )));
        }
        if maximum <= self.send_max {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_DATA value {maximum} does not increase the current maximum {}",
                self.send_max
            )));
        }
        self.send_max = maximum;
        Ok(())
    }

    /// 広告する受信上限を更新する (WT_MAX_DATA 送信前に呼び出す)
    ///
    /// draft-ietf-webtrans-http2-16 Section 6.5:
    /// 現在の広告値 (初期値を含む) を増加させない値は拒否する。
    /// また varint の最大値 (2^62 - 1) を超える値も拒否する
    /// (RFC 9000 Section 16)。
    ///
    /// このメソッドは capsule を出力しない内部状態更新であり、
    /// [`crate::webtransport::WtSession::send_max_data`] が内部で呼ぶ。
    /// `flow_control_mut()` 経由で直接呼ぶと広告されない状態更新になるため、
    /// `WT_MAX_DATA` の広告には `send_max_data` を使うこと。
    /// `WtStream::update_recv_max` は広告を行わない補助 API で同値を無視するのに対し、
    /// 本メソッドは広告値を進める更新であるため同値を拒否する。
    ///
    /// 旧 `add_recv_max(increment)` の置き換えであり、増分で指定する場合は
    /// 次のように呼び出す (加算は飽和させ、varint 上限超過は本メソッドが拒否する)。
    ///
    /// ```
    /// # use shiguredo_http2::webtransport::WtFlowControl;
    /// # let mut flow_control = WtFlowControl::new(0, 65536, 0, 0, 0, 0);
    /// let increment = 4096;
    /// flow_control
    ///     .update_recv_max(flow_control.recv_max().saturating_add(increment))
    ///     .expect("増加する上限は受理される");
    /// ```
    pub fn update_recv_max(&mut self, maximum: u64) -> WtResult<()> {
        // varint でエンコードできない値を保持すると WT_MAX_DATA の送信で panic するため
        if maximum > super::varint::MAX_VALUE {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_DATA value {maximum} exceeds varint maximum {}",
                super::varint::MAX_VALUE
            )));
        }
        if maximum <= self.recv_max {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_DATA value {maximum} does not increase the advertised maximum {}",
                self.recv_max
            )));
        }
        self.recv_max = maximum;
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

    /// `WT_MAX_DATA` を送信すべきかどうかを返す
    ///
    /// 受信ウィンドウが初期サイズの半分 (切り上げ) 未満になったら更新を推奨する。
    /// 切り捨てだと `initial_max_data == 1` でしきい値が 0 になり拡張されないため、
    /// 切り上げ除算を使う (`initial_max_data == 0` は 0 のまま拡張しない)。
    #[must_use]
    pub fn should_send_max_data(&self, initial_max_data: u64) -> bool {
        self.recv_available() < initial_max_data.div_ceil(2)
    }
}
