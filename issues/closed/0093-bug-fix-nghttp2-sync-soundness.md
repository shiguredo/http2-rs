# shiguredo_nghttp2::Session の unsafe impl Sync を見直す

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/fix-nghttp2-sync-soundness
- Polished: 2026-07-30

## 目的

`unsafe impl Sync for Session` の SAFETY コメントが実態と一致せず、`&self` メソッド経由で nghttp2 の C 関数が同時呼び出しされる UB の可能性がある問題を修正する。

## 現状

`crates/shiguredo_nghttp2/src/session.rs` の `unsafe impl Sync for Session` の SAFETY コメントは「全パブリックメソッドは `&mut self` を要求する」と述べている。しかし `want_write()` / `want_read()` / `get_remote_settings()` / `get_local_window_size()` 等は `&self` で nghttp2 の C 関数を呼び出している。nghttp2 はスレッドセーフではないため、複数スレッドから `&Session` を経由して同時にこれらのメソッドを呼ぶと UB の可能性がある。

## 完了条件

- `Sync` の健全性が保証されるか、`Sync` が削除されること
- `&self` メソッドの同時呼び出しが安全であることの根拠が SAFETY コメントに明記されること

## 解決方法

nghttp2 のドキュメントで `&self` 相当の読み取り専用関数のスレッド安全性を確認する。安全でなければ `unsafe impl Sync` を削除する。安全であれば SAFETY コメントに根拠を明記する。
