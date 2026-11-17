# Extended CONNECT の 2xx 後に HEADERS をストリームエラーにする

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-extended-connect-headers
- Polished: 2026-11-17

## 目的

RFC 8441 Section 4 の Extended CONNECT は、CONNECT と同様にトンネルを確立する。RFC 9113 Section 8.5 は connected stream で DATA / stream management 以外のフレームをストリームエラー (Section 5.4.2) として扱う MUST を定める。本実装は既存の CONNECT 確立済みストリーム処理と同じく `PROTOCOL_ERROR` を選択する。実装は Extended CONNECT を「通常の HTTP/2 ストリーム」として扱い、2xx 後の HEADERS を拒否しないため、仕様に合わせる。

## 現状

`src/connection/headers.rs` の `send_response` は 2xx 送信時に `Stream::set_connect_established` を呼ぶが、`Stream::has_protocol` が真の Extended CONNECT では呼ばない。同じファイルの `process_headers` のクライアントロール側も、2xx レスポンス受信時に同じ条件で `Stream::set_connect_established` を呼ばない。`src/stream.rs` の `has_protocol` の doc も「Extended CONNECT は通常の HTTP/2 ストリームとして動作するため、CONNECT トンネルのフレーム種別制限の対象外」としている。`src/stream.rs` の `connect_established` の doc は「通常の CONNECT (:protocol なし) で 2xx レスポンスが送受信された場合に true になる」としている。

受信側の `handle_headers` は `Stream::connect_established` が真の場合だけ `connect_established_headers_received` を立て、HEADERS をストリームエラーにする。

## 設計方針

- `Stream::connect_established` を Extended CONNECT の 2xx 後にも true にする (通常 CONNECT と同じ扱いにする)。`send_response` (サーバー) と `process_headers` (クライアント) の両方で `Stream::has_protocol` による除外を外す。
- Extended CONNECT の 2xx 後は `connect_established` を通じて HEADERS を `PROTOCOL_ERROR` のストリームエラーで拒否する。既存の `connect_established_headers_received` の遅延リセット機構をそのまま使う。
- `Stream::connect_established` の doc を「通常 CONNECT または Extended CONNECT の 2xx 後に true」に修正する。`Stream::has_protocol` の doc も、Extended CONNECT も CONNECT と同様にトンネルを確立するため 2xx 後はフレーム種別制限の対象となる旨に修正する。
- `src/connection/headers.rs` の `handle_headers` の「Extended CONNECT (:protocol 付き) は通常のストリームとして動作するため対象外」というコメントと、`send_response` / `process_headers` の「通常 CONNECT」というコメントを実装に合わせて修正する。
- `Stream::has_protocol` の getter は修正後にライブラリ内から呼ばれなくなる (`set_has_protocol` は引き続き使用する) ため、CODEBASE.md の公開 API 規約に従い、`set_has_protocol` の値を `has_protocol()` が返すことを検証するテストを追加する。
- Extended CONNECT の 2xx 後に HEADERS を送るテストを追加し、RST_STREAM (`PROTOCOL_ERROR`) と `Event::StreamReset` (connection_window_consumed: 0)、`streams` からの削除、接続維持を検証する。サーバー側 (`send_response` で 2xx) とクライアント側 (`process_headers` で 2xx 受信) の両方を対象とする。
- 通常の CONNECT の既存テストが維持されることを確認する。

## 完了条件

- Extended CONNECT の 2xx 後に HEADERS を受信すると、CONNECT ストリームが RST_STREAM (`PROTOCOL_ERROR`) で終了し、`Event::StreamReset` (connection_window_consumed: 0) が通知され、`streams` から削除され、接続が維持されること。サーバー側とクライアント側の両方で検証されること。
- `Stream::connect_established` と `Stream::has_protocol` の doc が実装と一致していること。`handle_headers` / `send_response` / `process_headers` の Extended CONNECT に関するコメントも実装と一致していること。
- Extended CONNECT 用の HEADERS テストが追加されていること。
- `Stream::has_protocol` の getter/setter のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
