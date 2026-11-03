# WT_MAX_STREAM_DATA をストリーム初期値から厳密に増加させる

- Created: 2026-11-03
- Completed: {YYYY-MM-DD}
- Branch: feature/change-wt-max-stream-data-strict-increase
- Polished: 2026-11-03

## 目的

WebTransport over HTTP/2 draft 16 Section 6.6 に従い、ストリームのデータ上限を増加させない `WT_MAX_STREAM_DATA` の送受信を拒否する。分類は後方互換のない仕様変更とする。

## 仕様差分

draft 15 の「直前の広告値より小さい値」に加え、draft 16 は同値も `WT_FLOW_CONTROL_ERROR` のセッションエラーとする MUST に変更した。最初の capsule も、そのストリームに適用される Section 4.3 の初期値を超える必要がある。

初期値は SETTINGS と `WebTransport-Init` の `u`・`bl`・`br` で決まり、開始主体とストリーム方向により異なる。Section 4.3.2 に追加された説明も、このヘッダーがストリームデータの初期上限を提供することを明確にしている。

一次資料: [draft-ietf-webtrans-http2-16 Section 6.6](https://datatracker.ietf.org/doc/html/draft-ietf-webtrans-http2-16#section-6.6)

## 現状

- `src/webtransport/stream.rs` の `WtStream::update_send_max` は `<` でのみ拒否する。`WtStream::new(0, 10, 10, true, true)` に `update_send_max(10)` を実行すると `Ok(())` になることを確認した。
- `src/webtransport.rs` の `WtSession::send_max_stream_data` も `maximum < stream.recv_max()` でのみ拒否し、コメントで同値の送信を明示的に許可している。初期値と同値の送信が成功することを確認した。
- 受信側は `WtSession::handle_capsule` から `WtStream::update_send_max` を呼ぶ。未知のピア開始 bidi ID では `create_implicit_peer_bidi_stream` によりストリームを生成してから上限を更新する。

## 設計方針

- 受信側の `update_send_max` と、wire 広告を行う `send_max_stream_data` で厳密な増加を要求する。拒否時は上限・出力を変更しない。
- 最初の受信値は、SETTINGS と `WebTransport-Init` を反映済みのストリームの送信上限と比較する。初期値 0 でも最初の値 0 を許可しない。
- クライアント・サーバー双方について、bidi は開始主体ごとの正しい初期値を使う。uni は、受信した上限を適用できるローカル開始 uni と、上限を広告できるピア開始 uni を区別する。ピア開始 bidi が capsule により暗黙に生成される経路も検証する。
- `grow_stream_recv_window` の正の増分は一度だけ反映し、増分 0 や飽和で値が増えない場合に capsule を生成しない。
- `WtStream::update_recv_max` 単体は wire 広告を行わない補助 API である。同値の無視という既存契約を一律に変更するのではなく、広告する境界で新規則を保証する。
- 既存の STOP_SENDING 後・受信終端後・送信専用ストリームに対する状態検証を維持する。

## 完了条件

- 初回・更新後の同値と減少が `WtErrorKind::FlowControlError` になり、増加した値だけが反映されること。
- SETTINGS とヘッダーで異なる初期値を与えた実セッションで、開始主体・方向ごとの比較基準を検証すること。
- 送信拒否時に出力・上限を変更せず、正常に広告した新上限まで受信できること。
- `tests/test_webtransport/stream.rs`、`tests/test_webtransport/integration.rs`、関連 PBT で検証し、tokio ドライバのストリーム受信ウィンドウ自動拡張が正常に動作すること。
- `cargo test -p shiguredo_http2`、`cargo test -p pbt` と関連する tokio の WebTransport テストが通ること。
