# closed_streams HashSet が永不変に増殖するメモリ枯渇問題を修正する

- Priority: High
- Created: 2026-06-06
- Model: DeepSeek V4 Pro

## 目的

`src/connection/mod.rs:68` の `closed_streams: HashSet<u32>` は、クローズ済みストリーム ID を追跡するが、一度追加されたエントリが**一度も削除されない**。長期間稼働する接続でストリーム ID 空間（最大約 21 億）の全エントリが蓄積されることで、メモリ枯渇 DoS が成立する。

## 優先度根拠

- 攻撃者が短命のストリームを大量に開閉することで、意図的にメモリを枯渇させられる
- RFC 9113 §10.5 は実装がリソース使用を監視し制限を設けることを SHOULD で要求している
- 特にサーバー側で長時間稼働する接続では、全クローズ済みストリーム ID が HashSet に残り続ける

## 現状

`src/connection/mod.rs` の以下の箇所でエントリが追加されるが、削除は一切行われない:

- `try_remove_closed_stream()` (line 767): 正常クローズ時
- `handle_data()` line 1010: DATA 受信でクローズ時
- `handle_rst_stream()` line 1031: RST_STREAM 受信時

`closed_streams` は「クローズ後に到着する遅延フレームの処理」に使用されるが、GOAWAY 受信後や `last_recv_stream_id` が大きく進んだ後は、古いエントリを削除しても問題ないはずである。

## 設計方針

以下のいずれかで対応する:

1. **GOAWAY 連動削除**: GOAWAY 受信時に `last_stream_id` より小さい全エントリを `closed_streams` から削除する
2. **last_recv_stream_id 連動削除**: ヘッダー受信時に `last_recv_stream_id` が更新された際、一定以上古いエントリを削除する
3. **上限付き**: `closed_streams` の最大エントリ数を設定し、超えた場合は古いエントリから削除する

RFC 9113 §5.1 の遅延フレーム処理要件を満たしつつ、メモリ消費を抑制する。

## 完了条件

- `closed_streams` からエントリが適切に削除される機構が実装されている
- 長時間稼働接続でもメモリが単調増加しない
- 既存の遅延フレーム処理テストが通過する
- 新規に `closed_streams` のエントリ削除を検証するテストが追加されている

## 解決方法

1. 作業ブランチ `feature/fix-closed-streams-memory-leak` を切る
2. GOAWAY 受信時に `last_stream_id` 以下のクローズ済みストリーム ID を `closed_streams` から削除する実装を追加する
3. テストを追加し `cargo test --all` で全通過を確認する
