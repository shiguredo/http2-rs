# 巨大な最大ストリーム数の広告で暗黙生成が大量の資源を消費する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-stream-id-resource-bounds
- Polished: 2026-11-17

## 目的

`create_peer_streams_up_to` は、ピアが開いたストリーム ID までの同型 ID をすべて生成する。`WtConfig::initial_max_streams_*` と `send_max_streams` は draft-ietf-webtrans-http2-16 Section 6.7 の 2^60 まで広告できるため、大きな上限を広告した状態で高いストリーム ID の capsule が届くと、1 回の処理で最大 2^60 件の `WtStream` と `WtEvent::StreamOpened` を生成し、大量のメモリと CPU を消費する。1 capsule あたりの暗黙生成数を実装上の安全上限で制限し、超過時は 1 件も生成せず拒否する。closed 0171 が決定した閉鎖記録の 10,000 件上限と既知制限は本 issue の対象外とし、変更しない。

## 現状

`src/webtransport.rs` の `create_peer_streams_up_to` は `first` から `stream_id` まで 4 ずつループし、`streams` に無く `closed_stream_send_max` にも記録されていない ID をすべて `WtStream` として登録して `WtEvent::StreamOpened` を積む。ループ回数は `(stream_id - first) / 4 + 1` であり、`stream_id` は varint の 2^62-1 まで、広告値は 2^60 まで取り得るため、1 回の呼び出しで最大 2^60 件を生成し得る。`WtFlowControl::can_accept_stream` は広告値に基づく上限だけを検証し、生成数の上限は検証しない。`send_max_streams` の doc はメモリ増加を「既知の制限」とするが、CPU の O(max_streams) ループと 2^60 までの広告を明示していない。closed 0171 は `STREAM_ID_RECORD_MAX_SIZE = 10000` と追い出しの既知制限を維持する決定をしており、その範囲は本 issue では扱わない。

## 設計方針

- `src/webtransport.rs` に `MAX_IMPLICIT_STREAMS: u64 = 10_000` を追加する。これは 1 個の受信 capsule で `create_peer_streams_up_to` が暗黙生成する同一型・同方向のストリーム数の上限である。`STREAM_ID_RECORD_MAX_SIZE` (閉鎖記録の 10,000 件上限) とは独立した定数であり、後者の値・追い出し方針・既知制限の doc は変更しない。
- `create_peer_streams_up_to` の冒頭で、`can_accept_stream` の検証後、ループ前に生成対象数を `(stream_id - first) / 4 + 1` で計算する。`MAX_IMPLICIT_STREAMS` を超える場合は 1 件も生成せず `WtError::flow_control_error` を返す。途中まで生成しないことで、エラー後にストリームと `WtEvent::StreamOpened` が残らないようにする (既存の `can_accept_stream` 拒否と同じ扱い)。
- 生成対象数は走査する範囲の大きさであり、既存ストリームと `closed_stream_send_max` に記録済みの ID を除いた実際の新規生成数ではない。既存 ID は生成しないが、ループの走査コストは範囲の大きさに比例するため、範囲の大きさで上限を判定する。
- 広告値の上限は仕様どおり 2^60 のまま変更しない (`src/webtransport/flow_control.rs` の `MAX_STREAMS_LIMIT` と `can_accept_stream` の判定も変更しない)。`WtConfig::initial_max_streams_*` の値も 2^60 以下という既存の前提を維持する。1 capsule あたりの生成上限は広告値とは別の制限である。
- 未使用の下位 ID の lazy 表現と広告値の clamp は採用しない。lazy 表現は `WtEvent::StreamOpened` の「下位 ID も通知する」契約を変える必要があり、clamp は `grow_max_streams` による広告値の増加と `tokio-http2` driver の `maybe_grow_max_streams` の再拡張と衝突するためである。1 capsule あたりの生成上限なら、広告値と driver の再拡張の挙動を変えずに資源消費を抑えられる。
- `MAX_IMPLICIT_STREAMS` の doc に、仕様の 2^60 と実装の 10,000 の関係、拒否条件、`STREAM_ID_RECORD_MAX_SIZE` との独立性を書く。`send_max_streams` / `grow_max_streams` の doc に、広告値は 2^60 までだが 1 capsule の暗黙生成は 10,000 件までであり、超過する stream ID の capsule は `flow_control_error` になることを書く。`create_peer_streams_up_to` の doc に、検証順 (広告値の `can_accept_stream` → 生成数上限) と、上限超過時に 1 件も生成しないことを書く。`WtConfig::initial_max_streams_*` の doc に、広告値と 1 capsule の生成上限は別であることを書く。`WtEvent::StreamOpened` の doc に、上限超過の capsule が拒否される場合は下位 ID の通知が行われないことを書く。
- README の WebTransport over HTTP/2 節または「制限 (DoS 対策)」節に、広告値は仕様上 2^60 まで、1 capsule の暗黙生成は 10,000 件まで、超過時は WT_FLOW_CONTROL_ERROR でセッションを終了する、閉鎖記録の 10,000 件上限は別の制限であることを書く。`skills/shiguredo-http2/SKILL.md` の WebTransport 節にも同じ上限を追記する。
- `tests/test_webtransport/integration.rs` に、高いストリーム ID を 1 個だけ送る回帰テストを追加する。`initial_max_streams_bidi` を 2^60 にしたサーバーセッションで、ピア (クライアント) 開始 bidi の `1 << 40` の WT_STREAM capsule を feed し、`process()` が `flow_control_error` を返し、ストリームが 1 件も生成されず `WtEvent::StreamOpened` も出ないことを検証する。境界として、同じセッションで `4 * 9_999` (10,000 件の暗黙生成) の WT_STREAM capsule が受理され、10,000 件のストリームと `StreamOpened` が生成されることも検証する。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しない。

## 完了条件

- `initial_max_streams_*` と `send_max_streams` / `grow_max_streams` の広告値が 2^60 まで大きくても、1 個の capsule で暗黙生成するストリーム数が `MAX_IMPLICIT_STREAMS` (10,000) を超えないこと。
- 生成対象数が上限を超える capsule は、1 件もストリームを生成せず `flow_control_error` を返すこと。エラー後に `WtStream` と `WtEvent::StreamOpened` が残らないこと。
- 広告値の上限 (2^60) と実装が保証する 1 capsule あたりの生成数の上限 (10,000)、拒否条件が rustdoc と README に明記されていること。`skills/shiguredo-http2/SKILL.md` の WebTransport 節も同じ内容になっていること。
- 高いストリーム ID を 1 個だけ送る回帰テストが追加され、上限超過の拒否と境界の受理が検証されていること。
- 既存の `STREAM_ID_RECORD_MAX_SIZE` (10,000) と追い出しの既知制限が変更されていないこと。
- `CHANGES.md` が変更されていないこと (`CODEBASE.md`)。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。

## スコープ外

- `STREAM_ID_RECORD_MAX_SIZE` (10,000) の値、追い出し方針、既知制限の doc (closed 0171 の決定) は変更しない。
- 広告値の 2^60 上限 (`src/webtransport/flow_control.rs` の `MAX_STREAMS_LIMIT`) と `WtFlowControl::can_accept_stream` の判定は変更しない。
- 未使用の下位 ID の lazy 表現と広告値の clamp は採用しない。

## 参照

- `refs/draft-ietf-webtrans-http2-16.txt` — Section 5.2 (WebTransport ストリーム ID と暗黙 open)、Section 6.7 (WT_MAX_STREAMS の 2^60 上限と暗黙 open)
- `refs/rfc9000.txt` — Section 2.1 (下位 ID の暗黙作成)、Section 4.6 (ストリーム数上限)
- `src/webtransport.rs` — `create_peer_streams_up_to` / `send_max_streams` / `grow_max_streams` / `WtConfig` / `WtEvent::StreamOpened`
- `src/webtransport/flow_control.rs` — `MAX_STREAMS_LIMIT` / `can_accept_stream`
- `src/webtransport/stream.rs` — `WtStreamId` / `stream_id::first` / `stream_id::next`
- `src/webtransport/varint.rs` — `MAX_VALUE` (2^62-1)
- `crates/tokio-http2/src/webtransport.rs` — `wt_http2_error_code` (FlowControlError → WT_FLOW_CONTROL_ERROR) / `maybe_grow_max_streams` (広告値の再拡張)
- `tests/test_webtransport/integration.rs` — 暗黙生成とストリーム数上限の既存テスト
- `skills/shiguredo-http2/SKILL.md` — WebTransport 節
- `issues/closed/0171-update-wt-stream-id-record-limit.md` — 閉鎖記録の 10,000 件上限と既知制限 (本 issue のスコープ外)
