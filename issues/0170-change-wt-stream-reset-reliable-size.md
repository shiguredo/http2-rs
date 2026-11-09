# WtEvent::StreamReset に Reliable Size を追加する

- Created: 2026-11-09
- Completed: 2026-11-09
- Branch: feature/change-wt-stream-reset-reliable-size
- Polished: 2026-11-09

## 目的

受信した `WT_RESET_STREAM` の Reliable Size をアプリへ伝える。draft-ietf-webtrans-http2-16 Section 6.2 は Reliable Size を「リセット前にピアが配信済みと宣言したバイト数」と定義し、受信側は受信済みバイト数 (`WtStream::recv_offset`) との一致を MUST で検証する。この値は検証後、匿名データとして捨てられているため、WT_RESET_STREAM で終端したストリームについて「リセット前に何バイト受信できたか」をアプリが知り得ない。イベントへ値を含め、ストリームの終端状態をアプリから観測できるようにする。

## 現状

- `src/webtransport.rs` の `WtEvent::StreamReset` は `{ stream_id, error_code }` のみを持つ
- `src/webtransport.rs` の `WtSession::handle_capsule` の `Capsule::WtResetStream` 分岐は `reliable_size != stream.recv_offset()` の一致検証後に値を捨て、`WtEvent::StreamReset` には `stream_id` と `error_code` だけを詰めて送出する
- `crates/tokio-http2/src/webtransport.rs` の `DriverState::dispatch_wt_event` は `WtEvent::StreamReset` の `stream_id` / `error_code` を使って `StreamPacket::Reset { error_code }` を流す。driver 内で消費する形であり、利用者への reliable_size 伝達はこの issue のスコープ外とする
- `src/webtransport/capsule.rs` の `Capsule::WtResetStream` と `pbt/tests/prop_webtransport` は capsule 自体の reliable_size は扱っており、取得自体は実装済み
- `tests/test_webtransport/integration.rs` の既存テストは `WtEvent::StreamReset { stream_id, .. }` で受けているため、フィールド追加はコンパイル上は既存テストを壊さない。値を検証するテストは存在しない

## 設計方針

- `WtEvent::StreamReset` に `reliable_size: u64` を追加する。enum variant のフィールド追加により利用側の網羅 match が壊れるため後方互換のない変更として扱う (`#[non_exhaustive]` は使わず、破壊的変更を明示する)
- 送出する値は、一致検証を通過した capsule の `reliable_size` とする。draft-ietf-webtrans-http2-16 Section 6.2 の MUST により `stream.recv_offset()` と常に一致するため、doc には「検証済みでありリセット前の受信済みバイト数と一致する」ことを明記する
- 送信側 (`WtSession::reset_stream` と WT_STOP_SENDING 受信時の自動応答) の変更は不要。reliable_size を `WtStream::send_offset()` から載せる現行の送信経路はそのまま
- `crates/tokio-http2/src/webtransport.rs` の `DriverState::dispatch_wt_event` の match は、既存の `stream_id` / `error_code` を名前で受け続け、`..` で第 3 フィールドを吸収して動作を変えない。`StreamPacket::Reset` 等の tokio 利用者表面への reliable_size 追加は別途検討する
- テストは `tests/test_webtransport/integration.rs` に追加する。既存の「reliable_size == recv_offset (正常)」のシナリオで、送出イベントの `reliable_size` が受信済みバイト数と一致すること、`reliable_size == 0 && recv_offset == 0` の正常シナリオで 0 が伝わることを検証する。過大・過小 (セッションエラー) の経路ではイベントが送出されない既存挙動をそのまま維持する
- PBT は既存の `pbt/tests/prop_webtransport` が capsule の reliable_size を扱っており、イベント面の検証は決定的なシナリオで足りるため追加しない

## スコープ外

- tokio 層の `StreamPacket::Reset` / `WtBidiStream` 等の利用者表面への reliable_size 追加
- HTTP/2 側の `Event::StreamReset` (RST_STREAM 用イベント) への変更
- RESET_STREAM_AT の導入や Reliable Size の部分配信意味論への対応。draft-ietf-webtrans-http2-16 Section 6.2 は WT_RESET_STREAM の設計思想として QUIC の RESET_STREAM_AT に言及するだけである境界を明示する

## 完了条件

- `WtEvent::StreamReset` が `{ stream_id, error_code, reliable_size }` になる
- WT_RESET_STREAM 受信時に `reliable_size` が受信済みバイト数と一致して送出されること、0 バイト終端時は 0 になることを検証するテストが `tests/test_webtransport/integration.rs` にある
- `crates/tokio-http2` の driver が `WtEvent::StreamReset` の追加フィールドを無視し、`StreamPacket` の構造が不変であることを確認すること (既存 tokio テストが通ること)
- `skills/shiguredo-http2/SKILL.md` の `WtEvent` 表に `reliable_size` を追記する
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `src/webtransport.rs` — `WtEvent::StreamReset` / `WtSession::handle_capsule` / `WtSession::reset_stream`
- `src/webtransport/stream.rs` — `WtStream::recv_offset` / `WtStream::recv_reset`
- `crates/tokio-http2/src/webtransport.rs` — `DriverState::dispatch_wt_event` / `StreamPacket::Reset`
- `tests/test_webtransport/integration.rs` — WT_RESET_STREAM の Reliable Size 一致検証テスト群
- `refs/draft-ietf-webtrans-http2-16.txt` — Section 6.2 (Reliable Size の定義と MUST equal)
- `skills/shiguredo-http2/SKILL.md` — `WtEvent` の表

## 解決方法

- `src/webtransport.rs` の `WtEvent::StreamReset` に `reliable_size: u64` を追加した。doc コメントに、検証を通過した値で `WtStream::recv_offset` と一致すること、受信専用単方向ストリームなどリセットでストリームが削除される経路では `WtSession::stream()` 参照が失われるため、受信集計をアプリで記録していない場合はリセット時の受信済みバイト数を知る唯一の経路であること (draft-ietf-webtrans-http2-16 Section 6.2) を記載した
- `WtSession::handle_capsule` の `Capsule::WtResetStream` 分岐で、一致検証 (`reliable_size != stream.recv_offset()`) を通過した capsule 値をイベントに載せて送出した。送信側 (`WtSession::reset_stream` と自動応答) は変更していない
- `crates/tokio-http2/src/webtransport.rs` の `DriverState::dispatch_wt_event` は `stream_id` / `error_code` を名前で受け、`..` で追加フィールドを吸収した。`StreamPacket::Reset` をはじめ利用者表面は不変であり、`..` 直前に「reliable_size は現行の利用者表面に載せていない」旨のコメントを残した
- `tests/test_webtransport/integration.rs` に正常系 3 件 (exact match / zero match / ピア開始 uni) で送出値の assert を追加し、過大・過小の 2 件に「StreamReset イベントが送出されないこと」の assert を追加した。あわせて、WT_STOP_SENDING 受信による自動 WT_RESET_STREAM 送信と追送データを組み合わせた組合検証テスト `wt_reset_stream_reliable_size_after_stop_sending_auto_reset` を追加した (受信集計 0 / 5 / 2 の全経路値を流水で固定する)
- `skills/shiguredo-http2/SKILL.md` の `WtEvent` 表の `StreamReset` 行に `reliable_size` を追記した
- WT_RESET_STREAM 関連テストの既存英語 expect メッセージを日本語に統一した
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ることを確認した
