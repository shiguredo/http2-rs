# Extended CONNECT の 2xx 後に HEADERS をストリームエラーにする

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-extended-connect-headers
- Polished: {YYYY-MM-DD}

## 目的

RFC 8441 Section 4 の Extended CONNECT は、CONNECT と同様にトンネルを確立する。RFC 9113 Section 8.5 は connected stream で DATA / stream management 以外のフレームを `PROTOCOL_ERROR` のストリームエラーとして扱う MUST を定める。実装は Extended CONNECT を「通常の HTTP/2 ストリーム」として扱い、2xx 後の HEADERS を拒否しないため、仕様に合わせる。

## 現状

`src/connection/headers.rs` は 2xx 送信時に `Stream::set_connect_established` を呼ぶが、`Stream::has_protocol` が真の Extended CONNECT では呼ばない。`src/stream.rs` の `has_protocol` の doc も「Extended CONNECT は通常の HTTP/2 ストリームとして動作するため、CONNECT トンネルのフレーム種別制限の対象外」としている。

受信側の `handle_headers` は `Stream::connect_established` が真の場合だけ `connect_established_headers_received` を立て、HEADERS をストリームエラーにする。

## 設計方針

- Extended CONNECT の 2xx 後も `connect_established` 相当の状態にし、HEADERS を `PROTOCOL_ERROR` のストリームエラーで拒否する。
- `Stream::has_protocol` の doc を修正する。
- Extended CONNECT の 2xx 後に HEADERS を送るテストを追加し、RST_STREAM (`PROTOCOL_ERROR`) を検証する。
- 通常の CONNECT の既存テストが維持されることを確認する。

## 完了条件

- Extended CONNECT の 2xx 後に HEADERS を受信すると、CONNECT ストリームが RST_STREAM (`PROTOCOL_ERROR`) で終了すること。
- `Stream::has_protocol` の doc が実装と一致していること。
- Extended CONNECT 用の HEADERS テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
