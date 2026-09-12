# 未作成のローカル開始 ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA が拒否されない

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-uncreated-local-stream-id-validation
- Polished: {YYYY-MM-DD}

## 目的

未作成のローカル開始ストリーム ID 宛の WT_STOP_SENDING / WT_MAX_STREAM_DATA を WT_STREAM_STATE_ERROR として拒否する。RFC 9000 Section 19.5 / Section 19.10 は「Receiving a ... for a locally initiated stream that has not yet been created MUST be treated as a connection error of type STREAM_STATE_ERROR」と定めている。0148 は受信専用 ID (ピア開始 uni) を静的判定で拒否したが、ローカル開始 ID は対象外だった。

## 現状

`WtSession::handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA は受信専用 ID (ピア開始 uni) のみを拒否し、未作成のローカル開始 ID は次のとおりすり抜ける。

- 未作成のローカル開始 ID への WT_STOP_SENDING はエラーにならず `WtEvent::StopSending` が送出される
- 未作成のローカル開始 ID への WT_MAX_STREAM_DATA は暗黙に無視される

ローカル開始 ID は `next_bidi_stream_id` / `next_uni_stream_id` が示す採番済み範囲より先かどうかで「未作成」を判定できる。

## 設計方針

- ロールと ID からローカル開始と判定できる ID のうち、`next_bidi_stream_id` / `next_uni_stream_id` が示す採番済み範囲に含まれない (まだ作成していない) ID への WT_STOP_SENDING / WT_MAX_STREAM_DATA を `WtError::stream_state_error` で拒否する
- 既に閉じて削除済みのローカル開始 ID (採番済み範囲内) への受信は従来どおり扱う
- 受信専用 ID (ピア開始 uni) への拒否とピア開始 bidi の未知 ID への従来挙動は非回帰とする
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- 未作成のローカル開始 bidi / uni ID への WT_STOP_SENDING が `stream_state_error` を返し、イベントが送出されないこと
- 未作成のローカル開始 bidi / uni ID への WT_MAX_STREAM_DATA が `stream_state_error` を返すこと
- 受信専用 ID とピア開始 bidi の未知 ID、開設済みローカル開始 ID への受信は従来どおり動作すること
- テストが追加され、`cargo test --all` が通過すること
