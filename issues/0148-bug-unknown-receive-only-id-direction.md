# ストリーム不在時の WT_STOP_SENDING / WT_MAX_STREAM_DATA が受信専用 ID を拒否しない

- Created: 2026-09-12
- Completed: 2026-09-12
- Branch: feature/fix-unknown-receive-only-id-direction
- Polished: 2026-09-12

## 目的

受信専用 (ピア開始 uni) のストリーム ID 宛の WT_STOP_SENDING / WT_MAX_STREAM_DATA は、対象ストリームが `WtSession` の `streams` に存在しない場合 (WT_STREAM 未受信・クローズ後の削除済み) に方向検証をすり抜け、エラーにならず受理される問題を修正する。RFC 9000 Section 19.5 / Section 19.10 は受信専用ストリームへの STOP_SENDING / MAX_STREAM_DATA に STREAM_STATE_ERROR を要求している。

## 現状

`WtSession::handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA は `self.streams.get(&stream_id)` が `Some` の場合のみ方向検証 (0146 で追加した `has_send_part()`) を行うため、ストリーム不在時は次のとおりすり抜ける。

- 受信専用 ID への WT_STOP_SENDING はエラーにならず `WtEvent::StopSending` が送出される (既存テスト `stop_sending_unknown_stream_emits_event` がこの挙動を固定している)
- 受信専用 ID への WT_MAX_STREAM_DATA は暗黙に無視される

ピア開始 uni で FIN 受信後に `remove_if_closed` で削除された ID も同じ経路になる。受信専用 ID はロールと ID の下位ビットから静的に判定できる。

## 設計方針

- ストリーム不在時も、ロールと ID から受信専用 (ピア開始 uni) と判定できる ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA を `WtError::stream_state_error` で拒否する
- ピア開始 bidi など、正当に先着し得る ID への受信は従来どおり扱う (WT_STOP_SENDING はイベント送出、WT_MAX_STREAM_DATA は無視)
- 既存テスト `stop_sending_unknown_stream_emits_event` はピア開始 bidi の ID に変更して挙動を維持し、受信専用 ID の拒否テストを追加する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- 受信専用 ID (ピア開始 uni) への WT_STOP_SENDING がストリーム不在時も `stream_state_error` を返し、イベントが送出されないこと
- 受信専用 ID (ピア開始 uni) への WT_MAX_STREAM_DATA がストリーム不在時も `stream_state_error` を返すこと
- ピア開始 bidi の ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA は従来どおり動作すること
- テストが追加され、`cargo test --all` が通過すること

## 解決方法

- `src/webtransport.rs` の `WtSession` に `is_peer_initiated()` / `is_receive_only_id()` を追加し、ロールと ID の下位ビットから受信専用 (ピア開始 uni) の ID を静的に判定できるようにした (RFC 9000 Section 2.1)。`handle_stream_data` の重複していた開始主体判定も `is_peer_initiated()` に共通化した
- `handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA の方向検証を ID ベースに置き換え、ストリームが未作成・削除済みでも受信専用 ID への受信を `WtError::stream_state_error` で拒否するようにした (draft-ietf-webtrans-http2-15 Section 5.2 / RFC 9000 Section 19.5 / Section 19.10)。拒否時はイベント送出・出力生成を行わない
- ピア開始 bidi など受信専用でない未知 ID への WT_STOP_SENDING はイベント送出、WT_MAX_STREAM_DATA は無視と、従来どおりの扱いを維持する
- `tests/test_webtransport/integration.rs` に未作成・削除済みの拒否テスト 4 件 (WT_STOP_SENDING / WT_MAX_STREAM_DATA、イベント非送出・出力非生成の検証を含む) と、ピア開始 bidi の非回帰テスト 1 件を追加した。既存の `stop_sending_unknown_stream_emits_event` はピア開始 bidi の ID (9997) を検証するテストに変更し、挙動維持を確認した
- `CHANGES.md` の `## develop` に `[FIX]` のエントリを追加した
