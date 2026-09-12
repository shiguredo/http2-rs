# 到達しないストリーム状態を削除する

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/remove-unreachable-stream-states
- Polished: 2026-09-12

## 目的

`RecvState` / `SendState` は QUIC の状態機械 (RFC 9000 Section 3.1 / Section 3.2) を mirror して 6 状態ずつ持っているが、このライブラリは HTTP/2 (TCP) 専用であり、順序配送により中間状態を経由せず即座に遷移する。到達不能な variant を削除し、公開する状態をこの実装が実際にとりうるものに合わせる。

## 現状

`RecvState` への代入は次の 4 箇所のみである。

- `WtStream::new` が `Recv`
- `WtStream::recv_data` が FIN 付き受信で `DataRecvd`
- `WtStream::recv_reset` が `ResetRead`
- `WtStream::mark_data_read` が `DataRead`

`SendState` への代入は次の 4 箇所のみである。

- `WtStream::new` が `Ready`
- `WtStream::send_data` が `Send`
- `WtStream::send_data` が FIN 送信で `DataRecvd`
- `WtStream::send_reset` が `ResetRecvd`

したがって削除対象は `RecvState::SizeKnown` / `RecvState::ResetRecvd` / `SendState::DataSent` / `SendState::ResetSent` の 4 variant である (`RecvState` と `SendState` のどちらにも `ResetRecvd` があるため、以下では常に修飾して区別する)。これら 4 variant を名指しする箇所は次のとおりで、テスト・PBT のコードと `examples/` は使用していない (テストが使う `ResetRecvd` は到達可能な `SendState::ResetRecvd` である)。

- enum の定義と doc の状態遷移図 (`src/webtransport/stream.rs`)
- `RecvState::can_recv` の `matches!` (`src/webtransport/stream.rs`)
- 中間状態を経由しない旨の実装コメント (`WtStream::send_data` / `WtStream::send_reset` / `WtStream::recv_data` / `WtStream::recv_reset`)
- `WtSession::stop_sending` の検証順序を説明するコメント (`src/webtransport.rs`。受信側の `RecvState::SizeKnown` と `RecvState::ResetRecvd` を名指ししている)
- `WtSession::check_max_stream_data_recv_state` の doc (`src/webtransport.rs`)
- `DriverState::maybe_grow_stream_window` のコメント (`crates/tokio-http2/src/webtransport.rs`)
- テストの doc コメント 2 箇所 (`tests/test_webtransport/integration.rs` の `stop_sending_data_recvd_accepted` / `stop_sending_data_read_accepted`。RFC 9000 Section 3.3 の受信側状態名として `ResetRecvd` を書いている)

`WtStream` の状態を設定する公開 API は無いため、利用者もテストもこれらの状態を再現できない。

到達しない理由は状態によって異なる。

- `SendState::DataSent` / `SendState::ResetSent` / `RecvState::SizeKnown`: draft-ietf-webtrans-http2-15 Section 5.2 が「Wherever QUIC relies on receiving an ack for a packet to transition between stream states, WebTransport performs that transition immediately.」とし、HTTP/2 の順序配送により FIN / RESET_STREAM の送受信時点で後続データの有無が確定するため、QUIC がもうける ACK 待ち・到着待ちの中間状態が不要になる
- `RecvState::ResetRecvd`: RFC 9000 Section 3.2 は「アプリケーションがリセットの通知を受け取ったとき」に `Reset Read` へ遷移するとするが、本実装は `WtStream::recv_reset` と `WtSession::handle_capsule` の `WtEvent::StreamReset` の送出を同時に行うため、リセット受信とアプリ通知を区別せず `ResetRead` へ直接遷移する

`CODEBASE.md` の「このライブラリの前提」は、本ライブラリが HTTP/2 と WebTransport over HTTP/2 を TCP/IP 上で扱うこと、および「本ライブラリで到達しない状態は公開 API に持たない」ことを定めている。また `CODEBASE.md` の「公開 API は必ず使用箇所とテストを用意すること」は「既存の未使用・テスト未使用の公開 API を発見した場合は、テストを追加して動作を保証するか、削除して解消すること」と定めている。これら 4 variant はテストで動作を保証できないため、削除して解消する。

また `RecvState::can_recv` が `SizeKnown` でも真になるため、「データを受け取れるか」と「MAX_STREAM_DATA を送れるか」(`Recv` 限定) が同じ述語で扱えるように見え、ドライバが誤った述語を使う余地が残っている (`issues/closed/0157-bug-driver-recv-state-check-size-known.md` で指摘し、本 issue の削除で解消する)。

## 設計方針

- `RecvState` を `Recv` / `DataRecvd` / `DataRead` / `ResetRead` の 4 状態にする
- `SendState` を `Ready` / `Send` / `DataRecvd` / `ResetRecvd` の 4 状態にする
- `RecvState::can_recv` は `Recv` のみ真にする。`RecvState::is_terminal` と `SendState::can_send` / `SendState::is_terminal` は変更しない (到達可能な状態だけを判定しており、削除の影響を受けない)
- enum の doc は状態遷移図の参照を残しつつ、本実装が経由しない状態とその理由を状態ごとに書き分ける。送信側 (`SendState`) は RFC 9000 Section 3.1 / Figure 2、受信側 (`RecvState`) は Section 3.2 / Figure 3 を参照しており、この節番号は変えない。理由は上記「現状」の書き分けに従う (順序配送による即座遷移と、リセット受信時にアプリ通知まで同時に行うこと)
- 中間状態を経由しない旨のコメント (`WtStream::send_data` / `WtStream::send_reset` / `WtStream::recv_data` / `WtStream::recv_reset`) と、`WtSession::stop_sending` の検証順序を説明するコメント、およびテストの doc コメント 2 箇所 (`stop_sending_data_recvd_accepted` / `stop_sending_data_read_accepted`) を、削除後も意味が通る表現に書き換える。削除後に `ResetRecvd` が残るのは送信側だけになるため、RFC 9000 の状態名は "Reset Recvd" / "Reset Read" と書き、Rust の variant 名 (`SendState::ResetRecvd`) と区別する
- `WtSession::check_max_stream_data_recv_state` の「`WtStream::can_recv()` は `SizeKnown` も許容するため、判定には使わない」という記述を、削除後の状態に合わせて更新する
- `crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` にある `SizeKnown` 前提のコメントを更新する。判定そのものは変更しない (削除により `WtStream::can_recv()` が `Recv` と同義になり、sans-io 層の `Recv` 限定検証と一致する)
- 公開 enum の variant 削除は後方互換のない変更である。`CODEBASE.md` の「この指示がなくなるまでは変更履歴を `CHANGES.md` に残さないこと」に従い `CHANGES.md` へのエントリは追加せず、後方互換の影響は本 issue と PR で伝える
- `issues/closed/0157-bug-driver-recv-state-check-size-known.md` は本 issue の削除で解消することを解決方法に記録して closed にした。本 issue の実装で 0157 に対して行う操作は無い

## 完了条件

- 到達しない 4 variant が削除され、`RecvState` と `SendState` がそれぞれ 4 状態になっていること
- `RecvState::can_recv` が `Recv` のみ真になり、`WtStream::can_recv()` と `WtSession::check_max_stream_data_recv_state` の `Recv` 限定判定が同じ意味になっていること
- 削除する 4 variant のうち `SizeKnown` / `DataSent` / `ResetSent` を名指しする記述がコード・コメント・doc に残っていないこと (`grep` で確認する)。`ResetRecvd` は到達可能な `SendState::ResetRecvd` が残るため、RFC 9000 の受信側状態を指す記述が削除後の状態を説明する表現になっていることをコードレビューで確認する
- enum の doc に、経由しない状態とその理由が状態ごとに書き分けられて書かれていること (自動テストでは検証できないため、コードレビューで確認する)
- 中間状態を経由しない旨のコメントと `WtSession::stop_sending` のコメントが、削除後の状態を説明する内容になっていること (コードレビューで確認する)
- `cargo test --all` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all --check` が通過すること
- `CHANGES.md` が変更されていないこと (`CODEBASE.md` の「この指示がなくなるまでは変更履歴を `CHANGES.md` に残さないこと」に従う)
