# is_idle_stream に Role を考慮するように変更する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-is-idle-stream-role-aware
- Polished: 2026-11-16

## 目的

`is_idle_stream` が `self.role` を参照せず、`streams` / `closed_streams` に存在しない偶数 ID を idle と判定する暗黙の前提を解消し、ロールに応じた idle 判定をシグネチャと実装で明示する。

## 現状

`is_idle_stream`（`src/connection.rs` の `Connection` 型の `is_idle_stream` メソッド）は、`streams` と `closed_streams` の確認後、`if stream_id.is_multiple_of(2) { return true; }` で残りの偶数 ID を idle と判定する。`streams` / `closed_streams` に存在する偶数 ID は先に非 idle と判定されるため、この分岐が対象とするのは `streams` / `closed_streams` に存在しない偶数 ID である。これはサーバープッシュ非サポートを前提とした実装判断であり、コードコメント `// サーバープッシュ非サポートのため偶数ストリーム ID は常にアイドル` にも明記されている。

しかし `is_idle_stream` は `self.role` を参照せず、ロールによって意味が変わる偶数 ID の扱いがコード上で明示されていない。RFC 9113 Section 5.1.1 は「クライアントが開始するストリームは奇数 ID を、サーバーが開始するストリームは偶数 ID を使用しなければならない (MUST)」と規定しており、偶数 ID はロールにかかわらずサーバー開始ストリームである。クライアントロールでは偶数 ID はピア（サーバー）が開始するストリームであり、サーバープッシュ非サポートのため未開設の偶数 ID は idle である。サーバーロールでは偶数 ID は自分（サーバー）が開始するストリームであり、サーバープッシュ非サポートのため未開設の偶数 ID は idle である。`streams` / `closed_streams` に存在する場合は、ロールにかかわらず先に非 idle と判定される。

現在はサーバープッシュ非サポートのため実害はないが、サーバープッシュをサポートする際に、偶数 ID を無条件に idle とみなす前提をロールに応じて見直す必要がある。

## 設計方針

- `is_idle_stream` に `Role` パラメータを追加し、呼び出し元は `self.role` を渡す。`Connection` は `self.role` を保持しているが、判定がロールに依存することをシグネチャで明示するために引数として渡す。
- `streams` / `closed_streams` に存在するストリームは従来どおり非 idle とする。
- 残りのストリームの判定は次のとおりにする。
  - クライアントロール: 偶数 ID はサーバー開始ストリームであり、サーバープッシュ非サポートのため idle。奇数 ID はクライアント開始ストリームであり、`last_recv_stream_id` を超える場合に idle。
  - サーバーロール: 奇数 ID はクライアント開始ストリームであり、`last_recv_stream_id` を超える場合に idle。偶数 ID はサーバー開始ストリームであり、サーバープッシュ非サポートのため idle。
- `check_not_idle_stream` も `is_idle_stream` を呼ぶため、`self.role` を渡すように修正する。`is_idle_stream` の呼び出し元は `Connection::process` / `Connection::reset_stream_internal` / `Connection::check_not_idle_stream` である。`check_not_idle_stream` は `handle_data` / `handle_rst_stream` / `handle_window_update` から呼ばれる。
- サーバープッシュ非サポートの範囲では、既存の idle 判定結果を変えない。特に、サーバーロールで暗黙的にクローズ済みになった奇数 ID（`last_recv_stream_id` 以下で `streams` / `closed_streams` に存在しないもの）を idle としない（`test_reset_stream_implicitly_closed_stream_sends_rst` が通ること）。サーバーロールの未開設の偶数 ID は idle のままとする（`test_reset_stream_on_even_stream_id_is_error` が通ること）。

## 完了条件

- `is_idle_stream` が `Role` を考慮し、`streams` / `closed_streams` に存在しないストリームについて、クライアントロールでは偶数 ID を idle、奇数 ID を `last_recv_stream_id` を超える場合に idle と判定し、サーバーロールでは奇数 ID を `last_recv_stream_id` を超える場合に idle、偶数 ID を idle と判定すること。
- `is_idle_stream` の呼び出し元（`Connection::process` / `Connection::reset_stream_internal` / `Connection::check_not_idle_stream`）が `self.role` を渡していること。
- サーバープッシュ非サポートの範囲で既存の idle 判定結果が変わらないこと（`test_reset_stream_on_even_stream_id_is_error` / `test_reset_stream_implicitly_closed_stream_sends_rst` を含む既存テストが通ること）。
- `cargo fmt --all -- --check` が通過すること。
- `cargo test --workspace` が全件通過すること。
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること。
