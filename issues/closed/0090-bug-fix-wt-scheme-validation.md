# Sans I/O 層で :protocol=webtransport 時の :scheme=https 検証を追加する

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/fix-wt-scheme-validation
- Polished: 2026-07-30

## 目的

`src/validation.rs` の `validate_request_headers` が Extended CONNECT の `:protocol` 値を参照せず、`:protocol=webtransport` + `:scheme=http` のリクエストが Sans I/O 層を通過する問題を修正する。

## 現状

`src/validation.rs` の `validate_request_headers` は `:protocol` の存在を `seen_protocol: bool` で記録するが、値自体を破棄している。Extended CONNECT に対して `:scheme` の存在は検証するが、`:scheme == "https"` の値検証は行わない。

draft-ietf-webtrans-http2-15 Section 3.2 は "The :scheme field MUST be https" と規定する。`tokio-http2` の `WtServerRequest::accept()` で `:scheme=https` を検証しているため、tokio-http2 経由では防げるが、Sans I/O ライブラリ直接利用時には MUST 違反となる。

## 設計方針

`validate_request_headers` で `:protocol` の値を保持し、`webtransport` の場合に `:scheme == "https"` を検証する。

## 完了条件

- `:protocol=webtransport` + `:scheme=http` が Sans I/O 層で拒否されること
- `:protocol=webtransport` + `:scheme=https` が正常に通過すること
- 単体テストが追加されていること

## 解決方法

`validate_request_headers` 内の `seen_protocol: bool` を `protocol_value: Option<&[u8]>` に変更し、Extended CONNECT の検証ブロックで `protocol_value == Some(b"webtransport")` かつ `scheme != "https"` の場合に `ValidationError` を返す。
