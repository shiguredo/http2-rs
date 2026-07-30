# wt_server README のステータスコードを実装に合わせる

- Created: 2026-07-30
- Completed: {Completed}
- Branch: feature/update-wt-server-readme-status-code
- Polished: 2026-07-30

## 目的

`examples/wt_server/README.md` に記載されている拒否ステータスコードが実装と不一致の問題を修正する。

## 現状

`examples/wt_server/README.md` には `WtServerRequest::reject(404)` および「全セッションを 404 で拒否」と記載されているが、実装（`examples/wt_server/src/main.rs` の `handle_connection` 関数内）では `req.reject(405)` を使用している。

## 完了条件

- README のステータスコードが実装の 405 と一致すること

## 解決方法

`examples/wt_server/README.md` の 404 の記載を 405 に修正する。
