# tokio-nghttp2

## 概要

shiguredo_nghttp2 (nghttp2 の Rust バインディング) を [Tokio](https://github.com/tokio-rs/tokio) と統合し、非同期 HTTP/2 クライアント/サーバーを提供するクレートです。

HTTP/2 のフレーム処理、HPACK、フロー制御は nghttp2 が担当し、TLS は [Rustls](https://github.com/rustls/rustls) (暗号ライブラリ: [aws-lc-rs](https://github.com/aws/aws-lc-rs)) で処理します。

## tokio-http2 との違い

tokio-http2 は Sans I/O な shiguredo_http2 (純 Rust 実装) を使用しているのに対し、tokio-nghttp2 は nghttp2 C ライブラリを使用しています。 TLS 処理は両者とも Rustls です。

## 依存ライブラリ

- [shiguredo_nghttp2](../shiguredo_nghttp2) - nghttp2 の Rust バインディング
- [Tokio](https://github.com/tokio-rs/tokio) - 非同期ランタイム
- [tokio-rustls](https://github.com/rustls/tokio-rustls) - Tokio 向け Rustls 統合
- [Rustls](https://github.com/rustls/rustls) - TLS 実装 (aws-lc-rs バックエンド)
- [rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier) - プラットフォームネイティブの証明書検証
- [rustls-pki-types](https://github.com/rustls/pki-types) - PKI 型定義

## API

### Client

HTTP/2 クライアントです。TLS 接続、リクエスト送信、レスポンス受信を行います。

#### 接続

- `Client::connect()` - サーバーに接続
- `Client::connect_with_options()` - `SessionOptions` 付きで接続
- `Client::connect_insecure()` - 証明書検証なしで接続 (テスト用)
- `Client::local_addr()` / `Client::remote_addr()` - 接続アドレスを取得

#### リクエスト送信

- `Client::send_request()` - リクエストを送信。引数は `(headers, data: Option<&[u8]>, end_stream)` で初期 DATA も同時送信可能
- `Client::send_data()` - ストリームに DATA を追加送信
- `Client::send_data_for_trailer()` - トレーラー前の最終 DATA を送信
- `Client::send_trailer()` - トレーラー HEADERS を送信

#### イベントループ

- `Client::poll_event()` - キューからイベントを取り出す
- `Client::next_event()` - イベントを待機
- `Client::drive()` - I/O とイベント処理を 1 回まわす
- `Client::recv()` - ネットワークから受信
- `Client::flush()` - 送信バッファをフラッシュ

#### コネクション制御

- `Client::ping()` - PING を送信
- `Client::shutdown()` - GOAWAY (NoError) を送信して接続を終了
- `Client::terminate()` - 任意の `ErrorCode` で GOAWAY を送信
- `Client::get_remote_settings()` / `Client::get_local_settings()` - SETTINGS 値を取得
- `Client::last_error_message()` - nghttp2 の最後のエラーメッセージを取得

### Server / ServerConnection

HTTP/2 サーバーです。TLS 接続の受け入れ、リクエスト受信、レスポンス送信を行います。

#### サーバー起動

- `Server::bind()` - アドレスにバインド
- `Server::accept()` - 接続を受け入れ
- `Server::accept_with_options()` - `SessionOptions` 付きで接続を受け入れ
- `Server::local_addr()` - バインドアドレスを取得

#### レスポンス送信

- `ServerConnection::send_response()` - レスポンス HEADERS を送信
- `ServerConnection::send_data()` - DATA を送信
- `ServerConnection::send_data_for_trailer()` - トレーラー前の最終 DATA を送信
- `ServerConnection::send_trailer()` - トレーラー HEADERS を送信
- `ServerConnection::send_headers()` - 追加 HEADERS (情報レスポンスなど) を送信

#### イベントループ

- `ServerConnection::poll_event()` - キューからイベントを取り出す
- `ServerConnection::next_event()` - イベントを待機
- `ServerConnection::drive()` - I/O とイベント処理を 1 回まわす
- `ServerConnection::recv()` - ネットワークから受信
- `ServerConnection::flush()` - 送信バッファをフラッシュ

#### コネクション制御

- `ServerConnection::reset_stream()` - RST_STREAM を送信
- `ServerConnection::shutdown()` - `last_stream_id` を指定して GOAWAY (NoError) を送信
- `ServerConnection::shutdown_graceful()` - graceful shutdown 通知 (`last_stream_id = (1 << 31) - 1` の GOAWAY) を送信
- `ServerConnection::terminate()` - 任意の `ErrorCode` で GOAWAY を送信
- `ServerConnection::get_remote_settings()` / `ServerConnection::get_local_settings()` - SETTINGS 値を取得
- `ServerConnection::last_error_message()` - nghttp2 の最後のエラーメッセージを取得
- `ServerConnection::local_addr()` / `ServerConnection::remote_addr()` - 接続アドレスを取得

### 再エクスポート

- `Connection<S>` - 任意の `AsyncRead + AsyncWrite` ストリーム上に HTTP/2 を載せる低レベル型
- `shiguredo_nghttp2` からの再エクスポート: `ErrorCode` / `FrameType` / `Header` / `Http2Event` / `SessionOptions` / `SettingsId` / `StreamId`
- `CONNECTION_PREFACE` - HTTP/2 コネクションプリフェイス定数

### TLS 設定

- `TlsClientConfig::with_platform_verifier()` - プラットフォームの証明書検証器を使用
- `TlsClientConfig::with_custom_ca()` - カスタム CA 証明書を使用
- `TlsClientConfig::insecure()` - 証明書検証を無効化 (テスト用)
- `TlsServerConfig::new()` - PEM 形式の証明書と秘密鍵から作成
- `TlsServerConfig::from_der()` - DER 形式の証明書と秘密鍵から作成

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
