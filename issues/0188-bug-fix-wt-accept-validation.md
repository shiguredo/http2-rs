# `WtServerRequest::accept` のリクエスト検証とエラー応答を統一する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-accept-validation
- Polished: 2026-11-17

## 目的

`WtServerRequest::from_connection` は `HeadersReceived` の `end_stream` を保持しないため、`accept()` 単体では要求が既に half-close されているかを検証できない。また `accept()` は `:method=CONNECT` / `:protocol=webtransport` を明示的に検査せず、Content-Length / Content-Type / Transfer-Encoding も検査しない。`selected_protocol` の検証失敗時は 4xx も RST_STREAM も送らず、`reject()` は `100..=599` を有効とするため `reject(200)` が 2xx + END_STREAM を送る。受理・拒否の境界検証とエラー応答を統一する。Origin 検証は本 issue の対象外とする。

## 現状

- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest` は `conn` / `stream_id` / `headers` のみを保持する。`HeadersReceived` の `protocol` と `end_stream` は捨てられる。
- `accept()` は `:scheme=https` と TLS 1.3 を検査するが、`:method=CONNECT` / `:protocol=webtransport` / `end_stream=false` を検査しない。`:method` と `:protocol` の組合せは Sans I/O 層の `validate_request_headers` が検証するが、`:protocol` の値が `webtransport` であることと `end_stream` は検証しない。
- `accept()` は Content-Length / Content-Type / Transfer-Encoding を検査しない。RFC 9297 Section 3.2 は Capsule Protocol を Content-Length / Content-Type / Transfer-Encoding 付きのメッセージで使うことを MUST NOT とし、違反を malformed として扱う MUST を定める。Transfer-Encoding は Sans I/O 層の `validate_request_headers` が `forbidden_headers::TRANSFER_ENCODING` として既に拒否するが、Content-Length と Content-Type は `accept()` に到達する。
- `selected_protocol` が `WT-Available-Protocols` にない場合、`Err(InvalidArgument)` を返すだけで接続を drop する。`selected_protocol` に ASCII printable 外のバイトが含まれる場合も同じくレスポンスを送らない。closed 0066 はリスト不在・不一致を「呼び出し側のミス」としてレスポンス送信なしと決定し、ASCII printable 外も現行実装はレスポンスを送らないが、`accept()` が `conn` を drop するためクライアントは接続断しか観測できない。
- `reject()` の `100..=599` 検査は 2xx を通し、1xx は `send_response` の END_STREAM 検査でエラーになる。範囲外は `Error::Io` を返す。

## 設計方針

- `WtServerRequest::from_connection` に `protocol: Option<Vec<u8>>` と `end_stream: bool` を追加し、`WtServerRequest` に保持する。`HeadersReceived` の `protocol` / `end_stream` をそのまま渡す。`from_connection` を `Result` にして Extended CONNECT 以外を構築不能にする案は、エラー応答を `accept()` に一元化できないため採用しない。
- `accept()` の検証順序と応答を次のように統一する。1〜4 は `RST_STREAM(PROTOCOL_ERROR)` で拒否する。TLS / `:scheme` / `:method` / `:protocol` / Content-* の違反は RFC 9113 Section 8.1.1 の malformed 要求の処理に従い、`end_stream` は本ライブラリの受理条件として同じ応答を使う。
  1. TLS 1.3 以外 → `RST_STREAM(PROTOCOL_ERROR)` + `Err(InvalidArgument)`（現行維持、draft-ietf-webtrans-http2-16 Section 7）
  2. `:scheme` が `https` 以外 → `RST_STREAM(PROTOCOL_ERROR)` + `Err(InvalidArgument)`（現行維持、draft-ietf-webtrans-http2-16 Section 3.2）
  3. `:method` が `CONNECT` 以外、`protocol` が `WEBTRANSPORT_PROTOCOL` (`b"webtransport"`) 以外、または `end_stream == true` → `RST_STREAM(PROTOCOL_ERROR)` + `Err(InvalidArgument)`（draft-ietf-webtrans-http2-16 Section 3.2 / RFC 8441 Section 4。`end_stream` は本ライブラリの受理条件）
  4. Content-Length / Content-Type / Transfer-Encoding のいずれかが存在 → `RST_STREAM(PROTOCOL_ERROR)` + `Err(InvalidArgument)`（RFC 9297 Section 3.2 の MUST NOT 違反を malformed として扱う）
  5. Origin 不一致 → `403` + `END_STREAM` + `Err(InvalidArgument)`（現行維持、draft-ietf-webtrans-http2-16 Section 3.2）
  6. `WebTransport-Init` パース失敗 → `400` + `END_STREAM` + `Err`（現行維持、draft-ietf-webtrans-http2-16 Section 4.3.2 の 4xx 要求）
  7. `selected_protocol` が ASCII printable 外、または `WT-Available-Protocols` に不在 → `400` + `END_STREAM` + `Err(InvalidArgument)`（draft-ietf-webtrans-http2-16 Section 3.3 のリスト内選択 MUST / 不適合時の拒否 MAY、ASCII printable は RFC 9651 Section 3.3.3）
  8. すべて通過 → `200`（`selected_protocol` 指定時は `wt-protocol` 付き、draft-ietf-webtrans-http2-16 Section 3.2 / Section 3.3）
- `reject()` の有効範囲を `300..=599` に限定する。`1xx` は `END_STREAM` を付けられず (RFC 9113 Section 8.1)、`2xx` は WebTransport セッション確立を意味する (draft-ietf-webtrans-http2-16 Section 3.2) ため、`reject()` では受け付けない。範囲外は `Error::InvalidArgument` を返し、レスポンスは送らない。`300..=599` は RFC 9110 Section 15 の有効なステータスコード範囲 (100..=599) に含まれる。
- Origin 検証は closed 0062 の `allowed_origin=None` スキップと closed 0085 の Origin 欠落時スキップを維持し、本 issue の対象外とする。
- Transfer-Encoding は Sans I/O 層で既に拒否されるため、`accept()` の検査は `from_connection` を直接呼ぶ利用者向けの防御とする。テストは直接構築した要求でも確認する。
- `CODEBASE.md` の指示により `CHANGES.md` は変更しない。

## 変更対象

- `crates/tokio-http2/src/webtransport.rs` — `WtServerRequest` のフィールド、`from_connection` のシグネチャ、`accept()` の検証と応答、`reject()` の範囲とエラー型、doc コメント
- `crates/tokio-http2/tests/test_webtransport.rs` — 既存の `from_connection` 呼び出しの更新、既存の `test_wt_selected_protocol_not_listed` の期待値・コメントの更新、各検証経路のテスト追加
- `examples/wt_server/src/main.rs` — `from_connection` 呼び出しの更新
- `crates/tokio-http2/README.md` — `from_connection` の例の更新
- `skills/shiguredo-http2/SKILL.md` — `from_connection` の例と `accept()` の挙動の更新

## 完了条件

- `:method` が `CONNECT` 以外、`protocol` が `webtransport` 以外、または `end_stream == true` の要求が `accept()` で `RST_STREAM(PROTOCOL_ERROR)` として拒否されること。
- Content-Length / Content-Type / Transfer-Encoding 付きの WT CONNECT が拒否されること。`accept()` に到達する Content-Length / Content-Type は `RST_STREAM(PROTOCOL_ERROR)` で拒否し、Transfer-Encoding は Sans I/O 層の既存拒否で満たす。
- `selected_protocol` が ASCII printable 外、または `WT-Available-Protocols` に不在の場合に `400` + `END_STREAM` が返ること。
- `reject(200)` / `reject(100)` / `reject(600)` が `Error::InvalidArgument` で拒否され、`reject(300)` / `reject(599)` が受理されること。
- 各エラー経路のテストが追加されていること。
- `WtServerRequest` / `from_connection` / `accept()` / `reject()` の doc コメントが新しいシグネチャと検証範囲に一致していること。
- `crates/tokio-http2/README.md` と `skills/shiguredo-http2/SKILL.md` の `from_connection` 例が新しいシグネチャに追従し、SKILL の `accept()` / `reject()` の挙動が新しい検証範囲に一致していること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
