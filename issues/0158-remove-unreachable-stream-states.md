# 到達しないストリーム状態を削除する

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/remove-unreachable-stream-states
- Polished: {YYYY-MM-DD}

## 目的

`RecvState` / `SendState` は QUIC の状態機械 (RFC 9000 Section 3.1 / Section 3.2) を mirror して 6 状態ずつ持っているが、このライブラリは HTTP/2 (TCP) 専用であり、順序配送により中間状態を経由せず即座に遷移する。到達不能な variant を削除し、公開する状態をこの実装が実際にとりうるものに合わせる。

## 現状

`RecvState` への代入は 4 箇所のみである。

- `WtStream::new` が `Recv`
- `WtStream::recv_data` が FIN 付き受信で `DataRecvd`
- `WtStream::recv_reset` が `ResetRead`
- `WtStream::mark_data_read` が `DataRead`

`SendState` への代入は 3 箇所のみである。

- `WtStream::new` が `Ready`
- `WtStream::send_data` が `Send`、FIN 送信で `DataRecvd`
- `WtStream::send_reset` が `ResetRecvd`

したがって `RecvState::SizeKnown` / `RecvState::ResetRecvd` / `SendState::DataSent` / `SendState::ResetSent` は代入されず、到達しない。これら 4 variant への参照は enum の定義、`RecvState::can_recv` の `matches!`、および「経由せずに遷移する」旨の実装コメントだけで、テスト・PBT・`crates/`・`examples/` からは参照されていない。`WtStream` の状態を設定する公開 API は無いため、利用者もテストもこれらの状態を再現できない。

到達しない理由は draft-ietf-webtrans-http2-15 Section 5.2 にある。同節は「Wherever QUIC relies on receiving an ack for a packet to transition between stream states, WebTransport performs that transition immediately.」とし、HTTP/2 の順序配送により FIN / RESET_STREAM の受信 (送信) 時点で後続データの有無が確定するため、QUIC がもうける中間状態が不要になる。

`CODEBASE.md` は「テスト・PBT で一度も使用されない公開 API を追加してはいけない」「既存の未使用・テスト未使用の公開 API を発見した場合は、テストを追加して動作を保証するか、削除して解消すること」と定めている。これら 4 variant はテストで動作を保証できないため、削除して解消する。

また `RecvState::can_recv` が `SizeKnown` でも真になるため、「データを受け取れるか」と「MAX_STREAM_DATA を送れるか」(`Recv` 限定) が同じ述語で扱えるように見え、ドライバが誤った述語を使う余地が残っている (`issues/0157-bug-driver-recv-state-check-size-known.md`)。

## 設計方針

- `RecvState` を `Recv` / `DataRecvd` / `DataRead` / `ResetRead` の 4 状態にする
- `SendState` を `Ready` / `Send` / `DataRecvd` / `ResetRecvd` の 4 状態にする
- `RecvState::can_recv` は `Recv` のみ真にする。`RecvState::is_terminal` と `SendState::can_send` / `SendState::is_terminal` は変更しない (到達可能な状態だけを判定しており、削除の影響を受けない)
- enum の doc は RFC 9000 Section 3.2 の状態遷移図を参照として残しつつ、本実装が経由しない状態 (`Data Sent` / `Size Known` / `Reset Sent` / 受信側の `Reset Recvd`) と、その理由 (draft-ietf-webtrans-http2-15 Section 5.2 の即座遷移と HTTP/2 の順序配送) を明記する
- `WtStream::send_data` / `WtStream::recv_data` / `WtStream::recv_reset` の「〜を経由せず」というコメントを、削除後も意味が通る表現に書き換える
- `WtSession::check_max_stream_data_recv_state` の「`WtStream::can_recv()` は `SizeKnown` も許容するため、判定には使わない」という記述を、削除後の状態に合わせて更新する
- `crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` にある `SizeKnown` 前提のコメントを更新する。判定そのものは変更しない (削除により `WtStream::can_recv()` が `Recv` と同義になり、sans-io 層の `Recv` 限定検証と一致する)
- 公開 enum の variant 削除は後方互換のない変更であるため、`CHANGES.md` の `## develop` に `[CHANGE]` を追加する
- `issues/0157-bug-driver-recv-state-check-size-known.md` が扱う食い違いは本 issue の削除で解消するため、0157 では実装しない

## 完了条件

- 到達しない 4 variant が削除され、`RecvState` と `SendState` がそれぞれ 4 状態になっていること
- `RecvState::can_recv` が `Recv` のみ真になり、`WtStream::can_recv()` と `WtSession::check_max_stream_data_recv_state` の `Recv` 限定判定が同じ意味になっていること
- enum の doc に、経由しない状態とその理由 (HTTP/2 の順序配送による即座遷移) が書かれていること
- `cargo test --all` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all --check` が通過すること
- `CHANGES.md` の `## develop` に `[CHANGE]` のエントリが追加されていること
