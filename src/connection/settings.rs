//! HTTP/2 SETTINGS フレーム処理
//!
//! SETTINGS パラメータの送受信と接続確立時の初期化を担当する。

use super::{Connection, ConnectionState, Role};
use crate::error::{Error, ErrorCode, Result};
use crate::event::Event;
use crate::frame::{Frame, SettingsFrame, StreamId};
use crate::settings::{Setting, DEFAULT_INITIAL_WINDOW_SIZE};

impl Connection {
    /// 接続プリフェイスを送信する（クライアント）
    ///
    /// RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 0 以外に設定できない。
    /// そのため、サーバーの場合は ENABLE_PUSH を送信しない。
    pub fn initiate(&mut self) -> Result<()> {
        if self.preface_sent {
            return Ok(());
        }

        if self.role == Role::Client {
            // クライアントはプリフェイス文字列を送信する
            self.output_buffer.extend(crate::CONNECTION_PREFACE);
        }

        // SETTINGS フレームを送信する
        let mut settings_frame = SettingsFrame::new();
        for setting in self.local_settings.to_settings_list() {
            // RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 1 に設定できない
            if self.role == Role::Server && matches!(setting, Setting::EnablePush(_)) {
                continue;
            }
            settings_frame.add(setting);
        }
        self.send_frame(&Frame::Settings(settings_frame))?;
        self.pending_settings_count += 1;
        self.preface_sent = true;

        // RFC 9113 Section 6.9.2: 接続レベルのウィンドウは SETTINGS では変更できないため
        // デフォルト (65535) を超える受信ウィンドウは WINDOW_UPDATE で広告する。
        self.send_initial_connection_window_update()?;

        Ok(())
    }

    /// SETTINGS フレームを送信する
    ///
    /// initiate() とは異なり、preface_sent のチェックを行わない。
    /// 外部でコネクションプリフェイスを処理した場合に使用する。
    ///
    /// RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 0 以外に設定できない。
    /// そのため、サーバーの場合は ENABLE_PUSH を送信しない。
    pub fn send_settings(&mut self) -> Result<()> {
        let mut settings_frame = SettingsFrame::new();
        for setting in self.local_settings.to_settings_list() {
            // RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 1 に設定できない
            if self.role == Role::Server && matches!(setting, Setting::EnablePush(_)) {
                continue;
            }
            settings_frame.add(setting);
        }
        self.send_frame(&Frame::Settings(settings_frame))?;
        self.pending_settings_count += 1;

        // RFC 9113 Section 6.9.2: 接続レベルのウィンドウは SETTINGS では変更できないため
        // デフォルト (65535) を超える受信ウィンドウは WINDOW_UPDATE で広告する。
        self.send_initial_connection_window_update()?;

        Ok(())
    }

    /// 接続確立時に接続レベル WINDOW_UPDATE を送信する
    ///
    /// `connection_window_size` がデフォルト (65535) より大きい場合のみ送信し、
    /// 初回送信後はフラグで二重送信を防ぐ。
    fn send_initial_connection_window_update(&mut self) -> Result<()> {
        if self.connection_window_update_sent {
            return Ok(());
        }
        if self.connection_window_size > DEFAULT_INITIAL_WINDOW_SIZE {
            let increment = self.connection_window_size - DEFAULT_INITIAL_WINDOW_SIZE;
            self.send_window_update(StreamId::Connection, increment)?;
        }
        // 既定値の場合でもフラグを立てて、後段の send_settings() で再評価しない。
        self.connection_window_update_sent = true;
        Ok(())
    }

    /// SETTINGS フレームを処理する
    pub(super) fn handle_settings(&mut self, frame: SettingsFrame) -> Result<()> {
        if frame.is_ack() {
            // RFC 9113 Section 6.5: 対応する SETTINGS がない ACK は接続エラー
            if self.pending_settings_count == 0 {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    "received SETTINGS ACK without pending SETTINGS",
                ));
            }
            // SETTINGS ACK を受信 (最古の未 ACK SETTINGS に対応)
            self.pending_settings_count -= 1;
            if self.state == ConnectionState::WaitingPreface {
                self.state = ConnectionState::Active;
            }
            self.events.push_back(Event::SettingsReceived { ack: true });
        } else {
            // SETTINGS を受信
            // RFC 9113 Section 6.5.2: SETTINGS_INITIAL_WINDOW_SIZE 変更時に
            // 既存ストリームのウィンドウサイズを調整する
            let old_initial_window_size = self.remote_settings.initial_window_size;

            // HEADER_TABLE_SIZE の変更を追跡
            let old_header_table_size = self.remote_settings.header_table_size;

            for setting in frame.settings() {
                // RFC 9113 Section 8.4: サーバーはクライアントに ENABLE_PUSH=1 を送信できない
                if self.role == Role::Client && matches!(setting, Setting::EnablePush(true)) {
                    return Err(Error::connection_error(
                        ErrorCode::ProtocolError,
                        "server sent ENABLE_PUSH=1 to client",
                    ));
                }

                // RFC 9218 Section 2.1: NO_RFC7540_PRIORITIES は接続中に変更できない
                if let Setting::NoRfc7540Priorities(new_value) = setting {
                    if let Some(initial_value) = self.initial_no_rfc7540_priorities {
                        if *new_value != initial_value {
                            return Err(Error::connection_error(
                                ErrorCode::ProtocolError,
                                "NO_RFC7540_PRIORITIES cannot be changed after initial setting",
                            ));
                        }
                    } else {
                        self.initial_no_rfc7540_priorities = Some(*new_value);
                    }
                }

                self.remote_settings.apply(*setting);
            }

            // RFC 9218 Section 2.1: NO_RFC7540_PRIORITIES は最初の SETTINGS フレームで
            // 送らなければならない (MUST)。最初の SETTINGS に含まれなかった場合、
            // デフォルト値 (0 = false) で確定し、以後の変更を拒否する。
            if self.state == ConnectionState::WaitingPreface
                && self.initial_no_rfc7540_priorities.is_none()
            {
                self.initial_no_rfc7540_priorities = Some(false);
            }

            // RFC 7541 Section 4.2: HEADER_TABLE_SIZE が変更された場合、
            // 次のヘッダーブロック送信時に Dynamic Table Size Update をエンコードする。
            // ヘッダーブロック間に複数回変化した場合、最小値と最終値の両方を送出する必要がある。
            let new_header_table_size = self.remote_settings.header_table_size;
            if new_header_table_size != old_header_table_size {
                let new_min = match self.pending_table_size_update {
                    Some((existing_min, _)) => existing_min.min(new_header_table_size),
                    None => new_header_table_size,
                };
                self.pending_table_size_update = Some((new_min, new_header_table_size));
            }

            // SETTINGS_INITIAL_WINDOW_SIZE が変更された場合、既存ストリームを更新
            let new_initial_window_size = self.remote_settings.initial_window_size;
            if new_initial_window_size != old_initial_window_size {
                self.update_stream_windows(new_initial_window_size)?;
            }

            // HPACK エンコーダーのテーブルサイズを更新
            self.hpack_encoder
                .set_max_table_size(self.remote_settings.header_table_size as usize);

            // RFC 9113 Section 4.2: 受信フレームサイズの上限はローカル設定で決まる。
            // remote_settings.max_frame_size は送信フレームの上限として使用する。
            // frame_decoder の max_frame_size はローカル設定で初期化済みなので更新不要。

            // SETTINGS ACK を送信
            self.send_frame(&Frame::Settings(SettingsFrame::ack()))?;

            self.events
                .push_back(Event::SettingsReceived { ack: false });

            if self.state == ConnectionState::WaitingPreface {
                self.state = ConnectionState::Active;
                self.events.push_back(Event::ConnectionPreface);
            }
        }

        Ok(())
    }

    /// SETTINGS_INITIAL_WINDOW_SIZE 変更時に既存ストリームのウィンドウサイズを調整する
    ///
    /// RFC 9113 Section 6.5.2: When the value of SETTINGS_INITIAL_WINDOW_SIZE changes,
    /// a receiver MUST adjust the size of all stream flow-control windows that it
    /// maintains by the difference between the new value and the old value.
    fn update_stream_windows(&mut self, new_initial_window_size: u32) -> Result<()> {
        for stream in self.streams.values_mut() {
            stream
                .flow_control_mut()
                .update_initial_window_size(new_initial_window_size)?;
        }
        Ok(())
    }
}
