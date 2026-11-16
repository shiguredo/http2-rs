# 16 MiB を超える WT_STREAM / DATAGRAM capsule を送信 API が生成し、受信側で拒否される

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-capsule-size-limit
- Polished: 2026-11-17

## 目的

`CapsuleDecoder` の既定バッファ上限は 16 MiB だが、`WtSession::send_datagram` / `send_stream_data` は 1 個の capsule のエンコード後サイズが 16 MiB を超えてもそのまま生成する。送信 API が正常に返しても、受信側は capsule のバイト列がバッファ上限を超えた時点で `InvalidInput` になり、デコードできない。送信側で 1 capsule のエンコード後サイズを検査し、送受信の契約を一致させる。

## 現状

`src/webtransport/capsule.rs` の `DEFAULT_MAX_BUFFER_SIZE` は 16 MiB で、`CapsuleDecoder::new()` と `WtSession` の受信側デコーダーはこの値を使う。`CapsuleDecoder::feed` はバッファ長が上限を超えると `InvalidInput`（reason: `capsule decoder buffer limit exceeded`）を返す。

`src/webtransport.rs` の `WtSession::send_datagram` と `send_stream_data` は capsule サイズの上限を検査せず、`Capsule::Datagram` / `Capsule::WtStream` をそのまま生成する。上限はデータ長ではなく capsule のエンコード後サイズで決まるため、次のようにデータ長が 16 MiB 未満でも失敗する。

- DATAGRAM: type 1 バイト + length 4 バイト + payload。payload が 16 MiB - 5 バイトを超えるとエンコード後サイズが 16 MiB を超える。16 MiB ちょうどの payload でも失敗する。
- WT_STREAM: type 4 バイト + length + Stream ID の varint + data。stream_id が 1 バイトの場合、data が 16 MiB - 9 バイトを超えると失敗する。Stream ID の varint が長いほどデータ上限は小さくなる。

16 MiB + 1 の DATAGRAM と、ピアの送信ウィンドウを 17 MiB 以上に広告した構成での 17 MiB の WT_STREAM を送信し、受信側の `CapsuleDecoder::feed` が `capsule decoder buffer limit exceeded` になることを確認済みである。DATAGRAM はフロー制御の対象外 (draft-ietf-webtrans-http2-16 Section 6.11) なので既定設定でも再現できるが、WT_STREAM は `send_stream_data` のフロー制御検査が先に働くため、ピアの `initial_max_data` / `initial_max_stream_data_*` をデータ長以上にし、`initial_max_streams_bidi` / `initial_max_streams_uni` も 1 以上にする必要がある。

## 設計方針

- 送信 API で capsule のエンコード後サイズを計算し、16 MiB を超える場合は既存の `flow_control_error` で拒否する。`WtErrorKind` は公開 enum で `#[non_exhaustive]` ではないため、新しい variant は追加しない。reason には capsule サイズ上限超過であることを明記する。
- 検査は `WtStream::send_data` / `WtFlowControl::consume_send` による状態更新と `output_buffer` への追加より前に行い、拒否時に送信状態・送信済みバイト数・出力を変更しない。
- `send_stream_data` のデータ長上限は、エンコード後サイズが 16 MiB 以下になる範囲とする。呼び出し側は複数の WT_STREAM に分割できるため、rustdoc に上限の計算方法（16 MiB から type / length / Stream ID の varint 長を引いた値）を明記する。
- `send_datagram` は分割できないため、エンコード後サイズが 16 MiB を超える場合は `flow_control_error` を返す。
- `WtSession` のデコーダ上限は本 issue では設定可能にしない。`CapsuleDecoder::new()` の既定値と送信側の検査値は共通の定数（`DEFAULT_MAX_BUFFER_SIZE` を `pub(crate)` にする等）を使う。
- `CapsuleDecoder` / `WtSession` の rustdoc と `README.md` に、1 capsule のエンコード後サイズ上限 16 MiB とデータ長の上限を明記する。tokio-http2 の公開送信 API の最大サイズと分割責任は 0183 の範囲とし、本 issue では `WtSession` と `README.md` を対象にする。
- エンコード後サイズ 16 MiB ちょうどの capsule が送受信でき、16 MiB + 1 の capsule が送信 API で拒否されるテストを追加する。WT_STREAM のテストはピアの `initial_max_data` / `initial_max_stream_data_*` をデータ長以上にし、`initial_max_streams_bidi` / `initial_max_streams_uni` も 1 以上に設定して、フロー制御で先に失敗しないようにする。
- 拒否時に出力・送信状態・送信済みバイト数が変化しないことをテストする。

## 完了条件

- 送信 API がエンコード後サイズ 16 MiB を超える capsule を生成せず、超過時は `flow_control_error` を返すこと。拒否時に送信状態・送信済みバイト数・出力が変化しないこと。
- `CapsuleDecoder` / `WtSession::feed` / `WtSession::send_datagram` / `WtSession::send_stream_data` の rustdoc、および `README.md` に、1 capsule のエンコード後サイズ上限 16 MiB とデータ長の上限が明記されていること。
- エンコード後サイズ 16 MiB ちょうどの capsule が送受信でき、16 MiB + 1 の capsule が送信 API で拒否されるテストが追加されていること。WT_STREAM のテストはフロー制御で先に失敗しないよう、ピアの送信ウィンドウをデータ長以上にし、`initial_max_streams_bidi` / `initial_max_streams_uni` も 1 以上に設定すること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
