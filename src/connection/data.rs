//! DATA フレームの送受信処理
//!
//! RFC 9113 Section 6.1 に基づく DATA フレームの送信（キューイング・
//! フラッシュ）と受信処理を提供する。

use super::Connection;
use crate::error::{Error, ErrorCode, Result};
use crate::event::Event;
use crate::frame::{DataFrame, Frame, NonZeroStreamId, StreamId};
use crate::stream::StreamState;

impl Connection {
    /// DATA フレームを送信する
    pub fn send_data(
        &mut self,
        stream_id: StreamId,
        data: Vec<u8>,
        end_stream: bool,
    ) -> Result<()> {
        let sid = stream_id.as_u32();

        // ストリームの存在確認と事前検証
        {
            let stream = self
                .streams
                .get_mut(&sid)
                .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

            // 既に END_STREAM をキューに積んだストリームへの追加 DATA は禁止。
            // state_machine の遷移は最後の DATA を実際に送信完了した時点で行うため
            // state はまだ Open/HalfClosedRemote のままだが、利用者の意図としては
            // 既に END_STREAM 宣言済みなので拒否する (RFC 9113 §5.1)。
            if stream.pending_end_stream() {
                return Err(Error::stream_error(
                    ErrorCode::StreamClosed,
                    "cannot send DATA after END_STREAM was queued for this stream",
                ));
            }

            // 状態チェック（end_stream=false でも送信可能な状態か検証する）
            stream.state_machine_mut().send_data(end_stream)?;
        }

        // データをキューに追加
        self.queue_data(sid, data, end_stream)?;

        // キューから送信可能な分を送信
        self.flush_stream_data(sid)?;

        // RFC 9113 Section 5.1: END_STREAM 付きフレームを実際に送信完了した場合のみ
        // ストリームを closed として削除する
        self.try_remove_closed_stream(sid);

        Ok(())
    }

    /// ストリームのデータをキューに追加する
    fn queue_data(&mut self, stream_id: u32, data: Vec<u8>, end_stream: bool) -> Result<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

        // 送信バッファにデータを追加
        let remaining = stream.send_buffer_mut().push(&data);
        if remaining > 0 {
            // バッファが満杯の場合はエラー
            return Err(Error::connection_error(
                ErrorCode::FlowControlError,
                "send buffer full",
            ));
        }

        // end_stream フラグを記録
        if end_stream {
            stream.set_pending_end_stream(true);
        }

        Ok(())
    }

    /// ストリームのキューからデータを送信する
    pub(super) fn flush_stream_data(&mut self, stream_id: u32) -> Result<()> {
        loop {
            // 送信可能なサイズを計算
            let (send_size, pending_end_stream) = {
                let stream = match self.streams.get(&stream_id) {
                    Some(s) => s,
                    None => return Ok(()),
                };

                let buffer_len = stream.send_buffer().len();
                let pending_es = stream.pending_end_stream();
                // RFC 9113 Section 6.9.1: フロー制御ウィンドウが 0 でも
                // END_STREAM 付きの長さ 0 DATA フレームは送信してよい
                if buffer_len == 0 && !pending_es {
                    return Ok(());
                }
                if buffer_len == 0 && pending_es {
                    break;
                }

                // 接続レベルのウィンドウ
                let conn_window = self.flow_control.send_available();
                // ストリームレベルのウィンドウ
                let stream_window = stream.flow_control().send_available();
                // 最大フレームサイズ
                let max_frame = self.remote_settings.max_frame_size as usize;

                // 送信可能なサイズ（ウィンドウとフレームサイズの最小値）
                let available = conn_window.min(stream_window).min(max_frame);

                if available == 0 {
                    // ウィンドウが枯渇している場合は送信しない
                    return Ok(());
                }

                let send_size = buffer_len.min(available);
                let pending_end_stream = stream.pending_end_stream();

                (send_size, pending_end_stream)
            };

            // データを取り出す
            // 直前の get で存在を確認済みのため expect で安全
            let data = {
                let stream = self.streams.get_mut(&stream_id).expect("stream must exist");
                stream.send_buffer_mut().pop(send_size)
            };

            // end_stream フラグを決定（バッファが空になり、pending_end_stream が true の場合）
            let remaining_after = {
                let stream = self.streams.get(&stream_id).expect("stream must exist");
                stream.send_buffer().len()
            };
            let end_stream = pending_end_stream && remaining_after == 0;

            // フロー制御を更新
            self.flow_control.consume_send(data.len())?;
            {
                let stream = self.streams.get_mut(&stream_id).expect("stream must exist");
                stream.flow_control_mut().consume_send(data.len())?;

                // end_stream を送信したらフラグをクリアし、状態機械を遷移させる
                if end_stream {
                    stream.set_pending_end_stream(false);
                    stream.state_machine_mut().complete_send_data(true)?;
                }
            }

            // DATA フレームを送信
            let sid =
                NonZeroStreamId::new(stream_id).expect("stream IDs in HashMap are always non-zero");
            let data_frame = DataFrame::new(sid, data).with_end_stream(end_stream);
            self.send_frame(&Frame::Data(data_frame))?;

            if end_stream || remaining_after == 0 {
                break;
            }
        }

        // RFC 9113 §6.9.1: 空 DATA + END_STREAM を送信する
        // ループから break で抜けた場合（buffer_len == 0 && pending_end_stream）
        if let Some(stream) = self.streams.get_mut(&stream_id)
            && stream.pending_end_stream()
            && stream.send_buffer().is_empty()
        {
            stream.set_pending_end_stream(false);
            stream.state_machine_mut().complete_send_data(true)?;
            let sid =
                NonZeroStreamId::new(stream_id).expect("stream IDs in HashMap are always non-zero");
            let data_frame = DataFrame::new(sid, vec![]).with_end_stream(true);
            self.send_frame(&Frame::Data(data_frame))?;
        }

        Ok(())
    }

    /// 全ストリームのキューからデータを送信する
    pub(super) fn flush_all_stream_data(&mut self) -> Result<()> {
        // 送信待ちデータまたは送信待ち END_STREAM があるストリームを収集
        let stream_ids: Vec<u32> = self
            .streams
            .iter()
            .filter(|(_, s)| !s.send_buffer().is_empty() || s.pending_end_stream())
            .map(|(id, _)| *id)
            .collect();

        for stream_id in stream_ids {
            self.flush_stream_data(stream_id)?;
            self.try_remove_closed_stream(stream_id);
        }

        Ok(())
    }

    /// DATA フレームを受信して処理する
    pub(super) fn handle_data(&mut self, frame: DataFrame) -> Result<()> {
        let sid = frame.stream_id.as_u32();

        // RFC 9113 Section 5.1: アイドルストリームへのフレームは接続エラー
        self.check_not_idle_stream(sid, "DATA")?;

        // RFC 9113 Section 6.1: フロー制御はペイロード全体に適用
        // (Pad Length フィールド + データ + パディング)
        let flow_control_size = if let Some(pad_length) = frame.pad_length {
            1 + frame.data.len() + pad_length as usize
        } else {
            frame.data.len()
        };

        // RFC 9113 Section 6.9: フロー制御フレームの受信者は、接続エラーとして扱わない限り、
        // 常に接続フロー制御ウィンドウに計上しなければならない (MUST)。
        // ストリームエラーで落とす場合でも接続ウィンドウは減算する必要がある。
        self.flow_control.consume_recv(flow_control_size)?;

        // RFC 9113 Section 5.1: Closed 状態のストリームへの DATA は最小処理して破棄する。
        // 接続フロー制御ウィンドウへの計上は上記で完了済み。
        // check_not_idle_stream を通過してマップにないストリームは暗黙的にクローズ済み。
        if self.is_stream_closed(sid) || !self.streams.contains_key(&sid) {
            return Ok(());
        }

        let is_closed = {
            let stream = self
                .streams
                .get_mut(&sid)
                .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

            stream.state_machine_mut().recv_data(frame.end_stream)?;
            stream.flow_control_mut().consume_recv(flow_control_size)?;

            // RFC 9113 Section 8.1.1: コンテンツを持たないレスポンス (204/304/HEAD) に
            // 内容を持つ DATA フレームが含まれている場合は malformed として扱う
            // (no-content の定義は RFC 9110 Section 6.4.1)
            if stream.no_content() {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    "DATA received on response defined as having no content (204/304/HEAD)",
                ));
            }

            // RFC 9113 Section 8.1.1: Content-Length とボディサイズの一貫性チェック
            let data_len = frame.data.len() as u64;
            stream.add_received_content_length(data_len);

            if let Some(expected) = stream.expected_content_length() {
                let received = stream.received_content_length();
                // 受信データが Content-Length を超過
                if received > expected {
                    return Err(Error::stream_error(
                        ErrorCode::ProtocolError,
                        format!(
                            "content-length mismatch: received {} exceeds expected {}",
                            received, expected
                        ),
                    ));
                }
                // END_STREAM 時に Content-Length と一致しない
                if frame.end_stream && received != expected {
                    return Err(Error::stream_error(
                        ErrorCode::ProtocolError,
                        format!(
                            "content-length mismatch: received {} but expected {}",
                            received, expected
                        ),
                    ));
                }
            }

            frame.end_stream && stream.state() == StreamState::Closed
        };

        let stream_id = StreamId::from(frame.stream_id);
        self.events.push_back(Event::DataReceived {
            stream_id,
            data: frame.data,
            end_stream: frame.end_stream,
        });

        if is_closed {
            self.events.push_back(Event::StreamClosed { stream_id });
            self.closed_streams.insert(sid);
            self.streams.remove(&sid);
        }

        Ok(())
    }
}
