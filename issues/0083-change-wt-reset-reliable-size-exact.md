# WT_RESET_STREAM の Reliable Size を受信バイトと一致必須にする

- Priority: High
- Created: 2026-07-20
- Polished: {Polished}
- Model: Grok 4.5
- Branch: feature/change-wt-reset-reliable-size-exact

## 目的

draft-ietf-webtrans-http2-15 Section 6.2 の Reliable Size 要件（送信済み WT_STREAM バイト総量と **一致** MUST）に受信検証を合わせ、過小・過大いずれの不一致も session error `WT_STREAM_STATE_ERROR` にする。

## 優先度根拠

- draft-14: Reliable Size が受信済みより **小さい** 場合のみ session error。超過分の破棄を許容する叙述があった
- draft-15: HTTP/2 上は順序保証があるため Reliable Size は送信済み総量と **MUST equal**。小さい値は既達データと矛盾、大きい値は後続バイトを約束するが到着し得ない → いずれも session error `WT_STREAM_STATE_ERROR`
- 過小のみ拒否のままだと、過大 Reliable Size を受け入れる非準拠になる

## 現状

`src/webtransport/mod.rs`（WT_RESET_STREAM 処理）:

```rust
if reliable_size < stream.recv_offset() {
    return Err(WtError::stream_state_error(...));
}
```

- 送信側 `reset_stream` は `stream.send_offset()` を Reliable Size に載せており、送信経路は既に一致前提
- 受信側は `reliable_size > recv_offset` を許容している

## 設計方針

- 受信時: `reliable_size != stream.recv_offset()` なら `WtError::stream_state_error`（最終的に HTTP/2 側では `WT_STREAM_STATE_ERROR` / 現行の `WebtransportStreamStateError`）
- 送信側は既存の `send_offset()` 利用を維持し、回帰テストで一致を明示する
- draft-14 由来の「超過データ破棄」コメント・テストがあれば削除または draft-15 向けに書き換える
- 0082（FIN 極性）完了後に着手するのが安全（ストリーム送受信の期待バイトが変わるため）

## スコープ外

- FIN 極性そのもの（0082）
- エラーコードの Display 名変更（0084）。本 issue では既存の `stream_state_error` 経路を使う
- CLOSE reason / Origin（0085）

## 他 issue との関係

- **0082** の後に実施
- **0084** でエラーコード名が `WT_STREAM_STATE_ERROR` に揃う。本 issue のテストメッセージは種類ベースで書き、Display 文字列への過度な依存を避ける

## 変更対象ファイル一覧

- `src/webtransport/mod.rs` — Reliable Size 検証
- `tests/test_webtransport/` — 過大 Reliable Size の拒否テスト追加、過小は維持
- `CHANGES.md` develop

## 完了条件

- `reliable_size != recv_offset` でセッションエラーになる
- 送信側が常に `send_offset` と一致する Reliable Size を送ることをテストで確認する
- `cargo test --workspace` / clippy `-D warnings` が通る
- `CHANGES.md` にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 6.2（Reliable Size MUST equal、不一致は WT_STREAM_STATE_ERROR）
- `src/webtransport/mod.rs` — `reliable_size < stream.recv_offset()` 付近
- `src/webtransport/mod.rs` — 送信側 `reliable_size = stream.send_offset()`
