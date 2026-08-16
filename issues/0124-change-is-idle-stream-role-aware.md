# is_idle_stream に Role を考慮するように変更する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-is-idle-stream-role-aware
- Polished: {YYYY-MM-DD}

## 目的

`is_idle_stream` が `Role` を考慮せずに偶数 ID を無条件に idle 判定する暗黙の前提を解消し、シグネチャを `Role` 対応にする。

## 現状

`is_idle_stream`（`src/connection.rs` の `Connection` 型の `is_idle_stream` メソッド）は `if stream_id.is_multiple_of(2) { return true; }` で全ての偶数 ID を idle と判定する。これはサーバープッシュ非サポートを前提とした実装判断であり、コードコメントにも明記されている。

しかし `is_idle_stream` のシグネチャは `Role` を受け取らず、この前提が暗黙的である。RFC 9113 Section 5.1.1 は「サーバー開始ストリームは偶数 ID を使用しなければならない (MUST)」と規定しており、クライアントロールでは偶数 ID はサーバー開始ストリーム（idle）だが、サーバーロールでは偶数 ID はクライアント開始ストリーム（idle ではない）である。

現在はサーバープッシュ非サポートのため実害はないが、将来的にサーバープッシュをサポートする場合にバグの温床となる。

## 設計方針

- `is_idle_stream` に `Role` パラメータを追加する
- クライアントロールでは偶数 ID を idle 判定、サーバーロールでは奇数 ID を idle 判定する
- 呼び出し元の `self.role` を渡すように修正する

## 完了条件

- `is_idle_stream` が `Role` を考慮した判定を行うこと
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
