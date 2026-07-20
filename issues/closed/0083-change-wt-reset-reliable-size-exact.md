# WT_RESET_STREAM の Reliable Size を受信バイトと一致必須にする

- Priority: High
- Created: 2026-07-20
- Completed: 2026-07-20
- Polished: 2026-07-20
- Model: Grok 4.5
- Branch: feature/change-wt-reset-reliable-size-exact

## 目的

draft-ietf-webtrans-http2-15 Section 6.2 の Reliable Size 要件（送信済み WT_STREAM バイト総量と **一致** MUST）に受信検証を合わせ、過小・過大いずれの不一致も session error `WT_STREAM_STATE_ERROR` にする。

## 優先度根拠

- draft-14: Reliable Size が受信済みより **小さい** 場合のみ session error。超過分の破棄を許容する叙述があった
- draft-15: HTTP/2 上は順序保証があるため Reliable Size は送信済み総量と **MUST equal**。小さい値は既達データと矛盾、大きい値は後続バイトを約束するが到着し得ない → いずれも session error `WT_STREAM_STATE_ERROR`

## 現状

`src/webtransport/mod.rs`（WT_RESET_STREAM 処理、L655-663 付近）:

```rust
if reliable_size < stream.recv_offset() {
    return Err(WtError::stream_state_error(...));
}
```

- 送信側 `reset_stream` は `stream.send_offset()` を Reliable Size に載せており、送信経路は既に一致前提
- 受信側は `reliable_size > recv_offset` を許容している（draft-14 セマンティクス）
- 過小 (`reliable_size < recv_offset`) の拒否テストは既存テストに存在しない（新規追加が必要）
- L655-656 のコメント「reliable_size が既に受信したオフセットより小さい場合はセッションエラー」は意味が変わるため書き換え対象

## 設計方針

- 受信時: `reliable_size != stream.recv_offset()` なら `WtError::stream_state_error`（最終的に HTTP/2 側では `WT_STREAM_STATE_ERROR` / 現行の `WebtransportStreamStateError`）
- 送信側は既存の `send_offset()` 利用を維持し、回帰テストで一致を明示する
- L655-656 のコメントを draft-15 の「MUST equal」セマンティクスに合わせて書き換える（0074 の機械置換では意味の修正はされないため、本 issue で実施）
- L658-662 のエラーメッセージ文字列 `"WT_RESET_STREAM reliable_size {} is less than recv_offset {}"` も `!=` 変更に合わせて書き換える（過大ケースで "is less than" は意味的に誤りになる）
- draft-14 由来の「超過データ破棄」コメント・テストはコードベースに存在しない（確認済み）
- 0082（FIN 極性）完了後に着手するのが安全（ストリーム送受信の期待バイトが変わるため）

## スコープ外

- FIN 極性そのもの（0082）
- エラーコードの Display 名変更（0084）。本 issue では既存の `stream_state_error` 経路を使う
- CLOSE reason / Origin（0085）

## 他 issue との関係

- **0082** の後に実施
- **0084** でエラーコード名が `WT_STREAM_STATE_ERROR` に揃う。本 issue のテストメッセージは種類ベースで書き、Display 文字列への過度な依存を避ける

## 変更対象ファイル一覧

- `src/webtransport/mod.rs` — Reliable Size 検証（`<` → `!=`）、コメント書き換え
- `tests/test_webtransport/integration.rs` — 過大・過小 Reliable Size の拒否テスト追加、送信側一致の回帰テスト追加
- `CHANGES.md` develop

## 完了条件

- `reliable_size != recv_offset` でセッションエラーになる（過小・過大の両方）
- エッジケースのテスト: `reliable_size == recv_offset`（正常）、`reliable_size == recv_offset + 1`（過大、エラー）、`reliable_size == recv_offset - 1`（過小、エラー）、`reliable_size == 0 && recv_offset == 0`（正常）
- 送信側が常に `send_offset` と一致する Reliable Size を送ることをテストで確認する
- `cargo test --workspace` / clippy `-D warnings` が通る
- `CHANGES.md` にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 6.2（Reliable Size MUST equal、不一致は WT_STREAM_STATE_ERROR）
- `refs/draft-ietf-webtrans-http2-14.txt` Section 6.2（旧: 過小のみ session error、超過データ破棄許容）
- `src/webtransport/mod.rs` — `reliable_size < stream.recv_offset()` 付近
- `src/webtransport/mod.rs` — 送信側 `reliable_size = stream.send_offset()`

## 解決方法

`src/webtransport/mod.rs` の WT_RESET_STREAM 受信時 Reliable Size 検証を `reliable_size < stream.recv_offset()` から `reliable_size != stream.recv_offset()` に変更した。コメントとエラーメッセージも draft-15 の MUST equal セマンティクスに合わせて書き換えた。

`tests/test_webtransport/integration.rs` にテスト 5 件を追加した: reliable_size == recv_offset (正常)、reliable_size == 0 && recv_offset == 0 (正常)、reliable_size > recv_offset (過大、エラー)、reliable_size < recv_offset (過小、エラー)、送信側が send_offset と一致する Reliable Size を送る回帰テスト。
