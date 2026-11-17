# ブラウザ interop の bidiEcho / uniEcho が FIN 待ちでデッドロックする

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-browser-interop-fin
- Polished: 2026-11-17

## 目的

`interop/browser/index.html` の `roundTrip` は `writer.write(payload)` の後、`writer.close()` より先に `readAll()` でサーバーの FIN を待つ。`examples/wt_server` の `handle_bidi` / `handle_uni` はクライアントの FIN が届くまで FIN を送らない。両者が互いの FIN を待つため、`bidiEcho` は 20 秒でタイムアウトし、`uniEcho` は INFO のため失敗が合否に反映されない。interop テストを成立させる。

## 現状

`interop/browser/index.html` の `roundTrip` は以下の順で処理する。

1. `writer.write(payload)`
2. `readAll(stream.readable)` でサーバーの FIN を待つ
3. `writer.close()`

`examples/wt_server/src/main.rs` の `handle_bidi` は `bidi.recv()` が `None` になるまでループし、その後 `bidi.send(Vec::new(), true)` する。`handle_uni` も同様である。したがってクライアントが FIN を送るまでサーバーは FIN を送らない。

`handle_bidi` / `handle_uni` は、クライアントの FIN を受信すると `recv()` が `Some(Vec::new())` を返す。現在のループはこの空データも `bidi.send(data, false)` / `send_stream.send(data, false)` でエコーする。この空の WT_STREAM capsule はストリームを開始も終了もしないため、draft-ietf-webtrans-http2-16 Section 6.4 の「空の WT_STREAM capsule はストリームを開くか閉じる場合のみ使う」に反し、受信側はセッションエラーとして扱ってよい (MAY)。本実装の `WtSession` は `empty WT_STREAM capsule without FIN on existing stream` として拒否する。クライアントが FIN を先に送る設計にすると、サーバーはこの空データをエコーしてから FIN を送るため、クライアントは空の非 FIN capsule を受ける。

`interop/browser/run.mjs` の `expectedNames` は `bidiEcho` を含むが、`uniEcho` は INFO であり、失敗しても `failed` にならない。

## 設計方針

- `interop/browser/index.html` の `roundTrip` は `writer.write(payload)` の直後に `writer.close()` し、その後 `readAll()` でエコーと FIN を待つ。サーバーはクライアントの FIN を受けてから FIN を送るため、先に送信側を閉じる必要がある。エコーは FIN より前に届く。
- `uniEcho` も `writer.write(payload)` の直後に `writer.close()` し、その後 `incomingUnidirectionalStreams` と `readAll()` でエコーと FIN を待つ。
- `examples/wt_server/src/main.rs` の `handle_bidi` / `handle_uni` は、`recv()` が返す空データ (FIN イベント) をエコーしない。ループ内で `data.is_empty()` なら `continue` し、ループを抜けた後に FIN を送る。これにより空の非 FIN WT_STREAM capsule を送らない。
- `uniEcho` を `infoChecks` から `checks` に移し、`run.mjs` の `expectedNames` に `uniEcho` を追加する。`infoChecks` には `bidiEcho4KiB` だけを残す。`uniEcho` が失敗した場合は `run.mjs` が失敗する。
- `bidiEcho4KiB` は `roundTrip` を使うため同じ順序になるが、WebKit の 4 KiB 制約の記録という目的を変えないため INFO のままにする。
- `interop/browser/README.md` の「送信側を閉じる順序」「検証項目」「実測結果」「既知の制約」を新しい順序に更新する。`index.html` の `roundTrip` / `uniEcho` / `uniSend` のコメントも更新する。
- `bidiEcho` / `uniEcho` を WebKit で実行し、タイムアウトしないことを確認する。

## 完了条件

- `bidiEcho` が FIN 待ちでタイムアウトしないこと。
- `uniEcho` の失敗が interop の合否に反映されること (`uniEcho` が `checks` に含まれ、`run.mjs` の `expectedNames` に追加されていること)。
- `handle_bidi` / `handle_uni` が空の非 FIN WT_STREAM capsule を送らないこと (FIN イベントの空データをエコーしないこと)。
- `interop/browser/README.md` の「送信側を閉じる順序」「検証項目」「実測結果」「既知の制約」と `index.html` の `roundTrip` / `uniEcho` / `uniSend` のコメントが新しい順序に更新されていること。
- WebKit の interop テストが通ること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
