# shiguredo_nghttp2::Session::send() が set_user_data() を呼ばない問題を修正する

- Priority: High
- Created: 2026-06-11
- Polished: {Polished}
- Model: deepseek-v4-pro
- Branch: feature/fix-nghttp2-send-set-user-data

## 目的

`shiguredo_nghttp2::Session::send()` が `set_user_data()` を呼ばないため、`send()` 時に data provider の read callback が発火すると `user_data` が null のまま `get_session()` が `None` を返し、`NGHTTP2_ERR_CALLBACK_FAILURE` (-902, fatal) が発生する問題を修正する。

## 現状の問題

`crates/shiguredo_nghttp2/src/session.rs:193-215`:

`send()` は `set_user_data()` を呼ばずに `nghttp2_session_mem_send` を実行する。一方 `recv()` (`session.rs:185-191`) は `self.set_user_data()` を呼んでいる。

通常フロー（`next_event()` が先に `recv()` を呼ぶ）では発生しないが、`next_event()` を経由せずに `send_request` / `send_data` を最初に呼ぶと、data provider callback 内で `get_session()` が `None` を返し、callback が `NGHTTP2_ERR_CALLBACK_FAILURE` を返してしまう。

## 完了条件

- `Session::send()` の先頭に `self.set_user_data()` 呼び出しが追加されていること
- `next_event()` を経由せずに `send()` を直接呼ぶテストケースが追加されていること
- CHANGES.md `## develop` に `[FIX]` エントリを追加すること

## 解決方法

`crates/shiguredo_nghttp2/src/session.rs:194` (`send()` の `self.output.clear();` の直後) に以下を追加する:

```rust
self.set_user_data();
```

## 参照

- `crates/shiguredo_nghttp2/src/session.rs:185` — `recv()` の `set_user_data()` 呼び出し（正しい例）
- `crates/shiguredo_nghttp2/src/session.rs:194` — `send()` で欠落している箇所
- `crates/shiguredo_nghttp2/src/session.rs:174-182` — `set_user_data()` の実装
- `crates/shiguredo_nghttp2/src/session.rs:743-749` — `get_session()` の実装（user_data が null の場合 None を返す）
