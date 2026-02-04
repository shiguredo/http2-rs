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

- `Session::client()` / `Session::server()` - セッションを作成
- `Session::recv()` - 受信データを処理
- `Session::send()` - 送信データを生成
- `Session::poll_event()` - イベントを取得
- `Session::submit_settings()` - SETTINGS フレームを送信
- `Session::submit_request()` - リクエストを送信 (クライアント、ヘッダーのみ。 DATA 送信は未実装)
- `Session::submit_response()` - レスポンスを送信 (サーバー、ヘッダーのみ。 DATA 送信は未実装)
- `Session::submit_rst_stream()` - RST_STREAM を送信
- `Session::submit_goaway()` - GOAWAY を送信
- `Session::submit_ping()` - PING を送信
- `Session::submit_window_update()` - WINDOW_UPDATE を送信

### イベント

- `HeadersReceived` - ヘッダー受信
- `DataReceived` - データ受信
- `StreamClosed` - ストリームクローズ
- `GoawayReceived` - GOAWAY 受信
- `PingReceived` - PING 受信
- `SettingsReceived` - SETTINGS 受信
- `WindowUpdateReceived` - WINDOW_UPDATE 受信

### 型

- `Header` - HTTP/2 ヘッダー (疑似ヘッダーのヘルパーメソッド付き)
- `ErrorCode` - HTTP/2 エラーコード (RFC 9113 Section 7)
- `FrameType` - HTTP/2 フレームタイプ (RFC 9113 Section 6)
- `StreamId` - ストリーム ID

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
