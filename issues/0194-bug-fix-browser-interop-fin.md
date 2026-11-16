# ブラウザ interop の bidiEcho / uniEcho が FIN 待ちでデッドロックする

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-browser-interop-fin
- Polished: {YYYY-MM-DD}

## 目的

`interop/browser/index.html` の `roundTrip` は `writer.write(payload)` の後、`writer.close()` より先に `readAll()` でサーバーの FIN を待つ。`examples/wt_server` の `handle_bidi` / `handle_uni` はクライアントの FIN が届くまで FIN を送らない。両者が互いの FIN を待つため、`bidiEcho` は 20 秒でタイムアウトし、`uniEcho` は INFO のため失敗が合否に反映されない。interop テストを成立させる。

## 現状

`interop/browser/index.html` の `roundTrip` は以下の順で処理する。

1. `writer.write(payload)`
2. `readAll(stream.readable)` でサーバーの FIN を待つ
3. `writer.close()`

`examples/wt_server/src/main.rs` の `handle_bidi` は `bidi.recv()` が `None` になるまでループし、その後 `bidi.send(Vec::new(), true)` する。`handle_uni` も同様である。したがってクライアントが FIN を送るまでサーバーは FIN を送らない。

`interop/browser/run.mjs` の `expectedNames` は `bidiEcho` を含むが、`uniEcho` は INFO であり、失敗しても `failed` にならない。

## 設計方針

- `roundTrip` は `writer.write(payload)` の直後に `writer.close()` し、その後 `readAll()` でエコーと FIN を待つ。エコーは FIN より前に届くため、順序は保たれる。
- `uniEcho` も write 直後に close する。
- `uniEcho` を INFO のままにせず、失敗を `run.mjs` の失敗条件に含める。
- `bidiEcho` / `uniEcho` を WebKit で実行し、タイムアウトしないことを確認する。

## 完了条件

- `bidiEcho` が FIN 待ちでタイムアウトしないこと。
- `uniEcho` の失敗が interop の合否に反映されること。
- WebKit の interop テストが通ること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
