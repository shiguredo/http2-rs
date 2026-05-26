//! ストリーム状態機械 (RFC 9113 Section 5.1)
//!
//! HTTP/2 ストリームの状態遷移を管理する。

use crate::error::{Error, ErrorCode};

/// ストリーム状態 (RFC 9113 Section 5.1)
///
/// ```text
///                              +--------+
///                      send PP |        | recv PP
///                     ,--------|  idle  |--------.
///                    /         |        |         \
///                   v          +--------+          v
///            +----------+          |           +----------+
///            |          |          | send H /  |          |
///     ,------| reserved |          | recv H    | reserved |------.
///     |      | (local)  |          |           | (remote) |      |
///     |      +----------+          v           +----------+      |
///     |          |             +--------+             |          |
///     |          |     recv ES |        | send ES     |          |
///     |   send H |     ,-------|  open  |-------.     | recv H   |
///     |          |    /        |        |        \    |          |
///     |          v   v         +--------+         v   v          |
///     |      +----------+          |           +----------+      |
///     |      |   half   |          |           |   half   |      |
///     |      |  closed  |          | send R /  |  closed  |      |
///     |      | (remote) |          | recv R    | (local)  |      |
///     |      +----------+          |           +----------+      |
///     |           |                |                 |           |
///     |           | send ES /      |       recv ES / |           |
///     |           | send R /       v        send R / |           |
///     |           | recv R     +--------+   recv R   |           |
///     | send R /  `----------->|        |<-----------'  send R / |
///     | recv R                 | closed |               recv R   |
///     `----------------------->|        |<-----------------------'
///                              +--------+
///
///     send:   endpoint sends this frame
///     recv:   endpoint receives this frame
///
///     H:  HEADERS frame (with implied CONTINUATIONs)
///     PP: PUSH_PROMISE frame (with implied CONTINUATIONs)
///     ES: END_STREAM flag
///     R:  RST_STREAM frame
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StreamState {
    /// アイドル状態（初期状態）
    #[default]
    Idle,
    /// ローカル側で予約済み（PUSH_PROMISE 送信後）
    ReservedLocal,
    /// リモート側で予約済み（PUSH_PROMISE 受信後）
    ReservedRemote,
    /// オープン状態（双方向通信可能）
    Open,
    /// ハーフクローズド（ローカル）- ローカル側から END_STREAM を送信済み
    HalfClosedLocal,
    /// ハーフクローズド（リモート）- リモート側から END_STREAM を受信済み
    HalfClosedRemote,
    /// クローズド状態
    Closed,
}

impl StreamState {
    /// ストリームがオープンかどうかを返す
    #[must_use]
    pub const fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    /// ストリームがクローズドかどうかを返す
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }

    /// ローカル側から送信可能かどうかを返す
    #[must_use]
    pub const fn can_send(&self) -> bool {
        matches!(self, Self::Open | Self::HalfClosedRemote)
    }

    /// リモート側からの受信が可能かどうかを返す
    #[must_use]
    pub const fn can_recv(&self) -> bool {
        matches!(self, Self::Open | Self::HalfClosedLocal)
    }
}

/// ストリーム状態機械
#[derive(Debug, Clone)]
pub struct StateMachine {
    /// 現在の状態
    state: StreamState,
}

impl StateMachine {
    /// 新しい状態機械を生成する（アイドル状態）
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: StreamState::Idle,
        }
    }

    /// 現在の状態を取得する
    #[must_use]
    pub const fn state(&self) -> StreamState {
        self.state
    }

    /// HEADERS フレーム送信時の状態遷移
    pub fn send_headers(&mut self, end_stream: bool) -> Result<(), Error> {
        self.state = match self.state {
            StreamState::Idle => {
                if end_stream {
                    StreamState::HalfClosedLocal
                } else {
                    StreamState::Open
                }
            }
            StreamState::ReservedLocal => {
                if end_stream {
                    StreamState::Closed
                } else {
                    StreamState::HalfClosedRemote
                }
            }
            StreamState::Open => {
                // トレーラーヘッダー送信
                if end_stream {
                    StreamState::HalfClosedLocal
                } else {
                    StreamState::Open
                }
            }
            StreamState::HalfClosedRemote => {
                // サーバーがレスポンスヘッダーまたはトレーラーを送信
                if end_stream {
                    StreamState::Closed
                } else {
                    StreamState::HalfClosedRemote
                }
            }
            _ => {
                return Err(Error::stream_error(
                    ErrorCode::StreamClosed,
                    "cannot send HEADERS in current state",
                ));
            }
        };
        Ok(())
    }

    /// HEADERS フレーム受信時の状態遷移
    pub fn recv_headers(&mut self, end_stream: bool) -> Result<(), Error> {
        self.state = match self.state {
            StreamState::Idle => {
                if end_stream {
                    StreamState::HalfClosedRemote
                } else {
                    StreamState::Open
                }
            }
            StreamState::ReservedRemote => {
                if end_stream {
                    StreamState::Closed
                } else {
                    StreamState::HalfClosedLocal
                }
            }
            StreamState::Open | StreamState::HalfClosedLocal => {
                // トレーラーヘッダー
                if end_stream {
                    if self.state == StreamState::HalfClosedLocal {
                        StreamState::Closed
                    } else {
                        StreamState::HalfClosedRemote
                    }
                } else {
                    self.state
                }
            }
            _ => {
                return Err(Error::stream_error(
                    ErrorCode::StreamClosed,
                    "cannot receive HEADERS in current state",
                ));
            }
        };
        Ok(())
    }

    /// DATA フレーム送信の事前検査
    ///
    /// 状態遷移は行わない。送信ウィンドウ枯渇でデータが内部バッファに残っている場合に、
    /// END_STREAM 付きの呼び出しでも state を Closed に進めないようにするため、
    /// validate (本メソッド) と state 遷移 (`complete_send_data`) を分離している。
    /// 状態遷移を先に進めると、送信完了前に Closed 扱いになり、ピアからの
    /// WINDOW_UPDATE が「Closed への WINDOW_UPDATE は無視」のルールで捨てられ、
    /// 永久に送信再開できなくなる (RFC 9113 §5.1 / §6.9.1)。
    pub fn send_data(&mut self, _end_stream: bool) -> Result<(), Error> {
        match self.state {
            StreamState::Open | StreamState::HalfClosedRemote => Ok(()),
            _ => Err(Error::stream_error(
                ErrorCode::StreamClosed,
                "cannot send DATA in current state",
            )),
        }
    }

    /// END_STREAM 付き DATA フレームを実際に送信完了したときの状態遷移
    ///
    /// `flush_stream_data` で最後のフレームを出力バッファに積んだ直後に呼ぶ。
    /// `end_stream == false` の場合は状態遷移なし。
    pub fn complete_send_data(&mut self, end_stream: bool) -> Result<(), Error> {
        if !end_stream {
            return Ok(());
        }
        match self.state {
            StreamState::Open => {
                self.state = StreamState::HalfClosedLocal;
                Ok(())
            }
            StreamState::HalfClosedRemote => {
                self.state = StreamState::Closed;
                Ok(())
            }
            _ => Err(Error::stream_error(
                ErrorCode::StreamClosed,
                "complete_send_data called in invalid state",
            )),
        }
    }

    /// DATA フレーム受信時の状態遷移
    pub fn recv_data(&mut self, end_stream: bool) -> Result<(), Error> {
        match self.state {
            StreamState::Open => {
                if end_stream {
                    self.state = StreamState::HalfClosedRemote;
                }
                Ok(())
            }
            StreamState::HalfClosedLocal => {
                if end_stream {
                    self.state = StreamState::Closed;
                }
                Ok(())
            }
            _ => Err(Error::stream_error(
                ErrorCode::StreamClosed,
                "cannot receive DATA in current state",
            )),
        }
    }

    /// RST_STREAM 送信時の状態遷移
    pub fn send_rst_stream(&mut self) {
        self.state = StreamState::Closed;
    }

    /// RST_STREAM 受信時の状態遷移
    pub fn recv_rst_stream(&mut self) {
        self.state = StreamState::Closed;
    }

    /// END_STREAM を送信したかどうかを返す
    #[must_use]
    pub const fn sent_end_stream(&self) -> bool {
        matches!(
            self.state,
            StreamState::HalfClosedLocal | StreamState::Closed
        )
    }

    /// END_STREAM を受信したかどうかを返す
    #[must_use]
    pub const fn received_end_stream(&self) -> bool {
        matches!(
            self.state,
            StreamState::HalfClosedRemote | StreamState::Closed
        )
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idle_to_open() {
        let mut sm = StateMachine::new();
        assert_eq!(sm.state(), StreamState::Idle);

        sm.send_headers(false).unwrap();
        assert_eq!(sm.state(), StreamState::Open);
    }

    #[test]
    fn test_idle_to_half_closed_local() {
        let mut sm = StateMachine::new();
        sm.send_headers(true).unwrap();
        assert_eq!(sm.state(), StreamState::HalfClosedLocal);
    }

    #[test]
    fn test_open_to_half_closed_local() {
        let mut sm = StateMachine::new();
        sm.send_headers(false).unwrap();
        sm.send_data(true).unwrap();
        // send_data は validate のみで状態遷移しない (フロー制御で詰まる可能性のため)
        assert_eq!(sm.state(), StreamState::Open);
        sm.complete_send_data(true).unwrap();
        assert_eq!(sm.state(), StreamState::HalfClosedLocal);
    }

    #[test]
    fn test_open_to_half_closed_remote() {
        let mut sm = StateMachine::new();
        sm.recv_headers(false).unwrap();
        sm.recv_data(true).unwrap();
        assert_eq!(sm.state(), StreamState::HalfClosedRemote);
    }

    #[test]
    fn test_half_closed_to_closed() {
        let mut sm = StateMachine::new();
        sm.send_headers(false).unwrap();
        sm.send_data(true).unwrap();
        sm.complete_send_data(true).unwrap();
        assert_eq!(sm.state(), StreamState::HalfClosedLocal);

        sm.recv_data(true).unwrap();
        assert_eq!(sm.state(), StreamState::Closed);
    }

    #[test]
    fn test_rst_stream_closes() {
        let mut sm = StateMachine::new();
        sm.send_headers(false).unwrap();
        sm.send_rst_stream();
        assert_eq!(sm.state(), StreamState::Closed);
    }

    #[test]
    fn test_server_response_from_half_closed_remote() {
        // クライアントが end_stream=true でリクエストを送信
        let mut sm = StateMachine::new();
        sm.recv_headers(true).unwrap();
        assert_eq!(sm.state(), StreamState::HalfClosedRemote);

        // サーバーがレスポンスを送信 (end_stream=true)
        sm.send_headers(true).unwrap();
        assert_eq!(sm.state(), StreamState::Closed);
    }

    #[test]
    fn test_server_response_with_body_from_half_closed_remote() {
        // クライアントが end_stream=true でリクエストを送信
        let mut sm = StateMachine::new();
        sm.recv_headers(true).unwrap();
        assert_eq!(sm.state(), StreamState::HalfClosedRemote);

        // サーバーがレスポンスヘッダーを送信 (end_stream=false)
        sm.send_headers(false).unwrap();
        assert_eq!(sm.state(), StreamState::HalfClosedRemote);

        // サーバーがボディを送信 (end_stream=true)
        sm.send_data(true).unwrap();
        // validate のみで遷移しない
        assert_eq!(sm.state(), StreamState::HalfClosedRemote);
        // 実際に送信完了したら Closed
        sm.complete_send_data(true).unwrap();
        assert_eq!(sm.state(), StreamState::Closed);
    }

    #[test]
    fn test_trailer_headers_from_open() {
        let mut sm = StateMachine::new();
        sm.send_headers(false).unwrap();
        assert_eq!(sm.state(), StreamState::Open);

        // トレーラーヘッダーを送信
        sm.send_headers(true).unwrap();
        assert_eq!(sm.state(), StreamState::HalfClosedLocal);
    }
}
