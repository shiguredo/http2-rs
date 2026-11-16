# 巨大な最大ストリーム数の広告で暗黙生成が大量の資源を消費する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-stream-id-resource-bounds
- Polished: {YYYY-MM-DD}

## 目的

`create_peer_streams_up_to` は、ピアが開いたストリーム ID までの同型 ID をすべて生成する。`WtConfig::initial_max_streams_*` と `send_max_streams` は 2^60 まで広告できるため、大きな上限を広告した状態で高いストリーム ID の capsule が届くと、1 回の処理で大量のメモリと CPU を消費する。暗黙生成の資源上限を仕様と実装の両面で整理する。closed 0171 が決定した閉鎖記録の 10,000 件上限と既知制限は本 issue の対象外とし、変更しない。

## 現状

`src/webtransport.rs` の `create_peer_streams_up_to` は `first` から `stream_id` まで 4 ずつループし、未作成 ID をすべて `WtStream` として登録して `WtEvent::StreamOpened` を積む。`send_max_streams` の doc はメモリ増加を「既知の制限」とするが、CPU の O(max_streams) ループと 2^60 までの広告を明示していない。

closed 0171 は `STREAM_ID_RECORD_MAX_SIZE = 10000` と追い出しの既知制限を維持する決定をしており、その範囲は本 issue では扱わない。

## 設計方針

- 実際に生成するストリーム数に上限を設ける、未使用の下位 ID を lazy に表現する、または広告値を実装上の安全上限で clamp する。
- `create_peer_streams_up_to` の生成数を入力に対して線形以上に増やさない、または上限超過を早期に拒否する。
- 高いストリーム ID を 1 個だけ送る DoS テストを追加する。
- 広告値の上限と、実装が保証する生成数の上限を README / rustdoc に明記する。

## 完了条件

- 広告した最大ストリーム数が大きくても、1 個の capsule で無制限のストリーム生成が起きないこと。
- 生成数の上限と拒否条件が doc に明記されていること。
- 高いストリーム ID を 1 個だけ送る回帰テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
