# shiguredo_nghttp2

## 概要

[nghttp2](https://nghttp2.org/) C ライブラリの Rust バインディングです。 nghttp2-sys の低レベル FFI の上に安全な Rust API を提供します。

nghttp2 が HTTP/2 のフレーム処理、HPACK 圧縮、フロー制御を担当します。

## 依存ライブラリ

- [nghttp2-sys](../nghttp2-sys) - nghttp2 への低レベル FFI バインディング
- [libc](https://github.com/rust-lang/libc) - C 型定義

## API

### Session

nghttp2 セッションをラップし、イベント駆動の HTTP/2 通信を提供します。

#### セッション作成

- `Session::client()` / `Session::server()` - セッションを作成
- `Session::client_with_options()` / `Session::server_with_options()` - `SessionOptions` 付きで作成
- `Session::role()` - 役割 (`SessionRole::Client` / `SessionRole::Server`) を取得

#### I/O とイベント

- `Session::recv()` - 受信データを処理
- `Session::send()` - 送信データを生成
- `Session::poll_event()` - イベントを取り出す
- `Session::want_read()` / `Session::want_write()` - 入出力が必要か判定
- `Session::last_error_message()` - nghttp2 が記録した最後のエラーメッセージを取得

#### フレーム送信

- `Session::submit_settings()` - SETTINGS フレームを送信
- `Session::submit_request()` - リクエスト (HEADERS + 任意で DATA) を送信。 `data: Option<&[u8]>` で初期 DATA を渡せる
- `Session::submit_response()` - レスポンス HEADERS を送信。 `end_stream = false` 時は後続 `submit_data()` で DATA を送る
- `Session::submit_data()` - ストリームに DATA を追加送信
- `Session::submit_data_for_trailer()` - トレーラー前の最終 DATA を送信 (`NGHTTP2_DATA_FLAG_NO_END_STREAM` を立てる)
- `Session::submit_trailer()` - トレーラー HEADERS を送信
- `Session::submit_headers()` - 既存ストリームに追加 HEADERS (情報レスポンスなど) を送信
- `Session::submit_rst_stream()` - RST_STREAM を送信
- `Session::submit_goaway()` - GOAWAY を送信
- `Session::submit_ping()` - PING を送信
- `Session::submit_window_update()` - WINDOW_UPDATE を送信
- `Session::submit_shutdown_notice()` - graceful shutdown 通知 (`last_stream_id = (1 << 31) - 1` の GOAWAY) を送信

#### セッション制御 / 状態取得

- `Session::terminate_session()` - GOAWAY を送ってセッションを終了
- `Session::get_remote_settings()` / `Session::get_local_settings()` - SETTINGS 値を取得
- `Session::get_outbound_queue_size()` - 送信待ちキューのサイズを取得
- `Session::get_next_stream_id()` / `Session::get_last_proc_stream_id()` - ストリーム ID を取得

#### フロー制御

- `Session::get_remote_window_size()` / `Session::get_local_window_size()` - コネクションウィンドウサイズを取得
- `Session::get_stream_remote_window_size()` / `Session::get_stream_local_window_size()` - ストリームウィンドウサイズを取得
- `Session::set_local_window_size()` - ローカルウィンドウサイズを設定 (`stream_id = 0` でコネクション全体)
- `Session::consume()` / `Session::consume_connection()` / `Session::consume_stream()` - 受信データ消費を通知 (`no_auto_window_update` 有効時)

### SessionOptions

DoS 対策やフロー制御の挙動を調整するための Builder です。

- `SessionOptions::new()` - 新規作成
- `no_auto_window_update()` - 自動 WINDOW_UPDATE を無効化
- `peer_max_concurrent_streams()` - ピア SETTINGS 受信前の最大同時ストリーム数の初期値
- `no_auto_ping_ack()` - 自動 PING ACK を無効化
- `max_send_header_block_length()` - 送信ヘッダーブロックの最大長
- `max_deflate_dynamic_table_size()` - HPACK deflate 動的テーブルの最大サイズ
- `max_outbound_ack()` / `max_settings()` / `max_continuations()` - DoS 対策の上限値
- `stream_reset_rate_limit()` / `glitch_rate_limit()` - レート制限 (burst, rate)

### イベント (`Http2Event`)

- `HeadersReceived` - HEADERS 受信
- `DataReceived` - DATA 受信
- `StreamClosed` - ストリームクローズ
- `GoawayReceived` - GOAWAY 受信
- `PingReceived` - PING 受信
- `SettingsReceived` - SETTINGS 受信
- `WindowUpdateReceived` - WINDOW_UPDATE 受信
- `FrameSent` - フレーム送信完了 (`on_frame_send` コールバック由来)
- `FrameNotSent` - フレーム送信失敗 (`on_frame_not_send` コールバック由来)
- `InvalidFrameReceived` - 不正フレーム受信 (`on_invalid_frame_recv` コールバック由来)
- `InvalidHeaderReceived` - 不正ヘッダー受信 (`on_invalid_header` コールバック由来)

### 型

- `Header` - HTTP/2 ヘッダー (`method()` / `scheme()` / `authority()` / `path()` / `status()` / `sensitive()` ヘルパー付き)
- `ErrorCode` - HTTP/2 エラーコード (RFC 9113 Section 7)
- `FrameType` - HTTP/2 フレームタイプ (RFC 9113 Section 6)
- `SettingsId` - HTTP/2 SETTINGS パラメータ ID (RFC 9113 Section 6.5.2, RFC 8441, RFC 9218)
- `StreamId` - ストリーム ID (`i32` のエイリアス)
- `SessionRole` - セッションの役割 (`Client` / `Server`)

### ユーティリティ

- `nghttp2_version()` - nghttp2 のバージョン文字列を取得
- `http2_strerror()` - nghttp2 エラーコードを文字列化
- `is_fatal()` - エラーが致命的か判定
- `check_header_name()` / `check_header_value_rfc9113()` / `check_method()` / `check_path()` / `check_authority()` - RFC 9113 準拠の検証ヘルパー

## nghttp2 ライセンス

<https://github.com/nghttp2/nghttp2/blob/master/COPYING>

```text
The MIT License

Copyright (c) 2012, 2014, 2015, 2016 Tatsuhiro Tsujikawa
Copyright (c) 2012, 2014, 2015, 2016 nghttp2 contributors

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## ライセンス

Apache License 2.0

```text
Copyright 2026-2026, Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
