# WT_MAX_STREAM_DATA をストリーム初期値から厳密に増加させる

- Created: 2026-11-03
- Completed: 2026-11-03
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

## 解決方法

- `WtStream::update_send_max` を、そのストリームの初期値または最後に受理した上限を増加させない値と varint 上限超過で `flow_control_error` を返すように変更した
- `WtSession::send_max_stream_data` を、初期値・最後に広告した上限を増加させない値 (同値・減少) で `flow_control_error` を返すように変更した。成功時は広告値と `WtStream::recv_max` を一致させる
- 比較基準の初期値は、開始主体・方向ごとに SETTINGS と `WebTransport-Init` の `u`・`bl`・`br` を反映した `WtConfig` (ローカル値は両者の大きい方) から決まる既存の経路を維持した。ピア開始 bidi の暗黙生成経路は生成前に同じ検証を行い、拒否時にストリームと `StreamOpened` を残さない
- 削除済みストリームでも draft-16 Section 6.6 の MUST を満たすため、初期値または最後に受理した送信上限を上限付きの `BoundedMap` で保持し、削除後の非増加を検出するようにした。削除後の増加は記録だけ更新して無視する。記録から追い出された ID は非増加の検出を行わない既知の制限がある
- `WtSession::grow_stream_recv_window` は存在確認と `checked_add` による u64 オーバーフロー検出だけを行い、状態検証・受信上限の更新・送信を `send_max_stream_data` に委譲するよう整理した。検証順とエラー種別を rustdoc に明記した
- `WtStream::send_max` と初期送信上限ヘルパーを追加し、暗黙生成の判定と生成で同じ条件・同じ初期値を使うようにした
- `src/bounded_map.rs` を追加し、追い出し・容量 0 / 1・同一キー更新の単体テストを追加した
- `tests/test_webtransport/stream.rs` に送信上限の同値・減少・varint 上限の拒否と `send_max` の反映を追加・更新した
- `tests/test_webtransport/integration.rs` に同値・減少・増分 0 の拒否、SETTINGS と `WebTransport-Init` の大きい方の初期値、開始主体・方向・uni・クライアントロールごとの初期値、暗黙生成の拒否と受理、検証順、削除済みストリームの非増加と増加の無視、varint 境界、受信 capsule の同値エラーを追加・更新した
- `pbt/tests/prop_webtransport/stream.rs` を新設し、`WtStream::update_send_max` の更新列、`send_max_stream_data` の広告列、`grow_stream_recv_window` の増分列、暗黙生成経路の PBT を追加した
- `skills/shiguredo-http2/SKILL.md` の `send_max_stream_data` / `grow_stream_recv_window` の検証順とエラー、暗黙生成と削除済みストリームの扱い、draft の適用範囲を更新した
- `CODEBASE.md` の指示に従い `CHANGES.md` は変更していない。`WT_MAX_STREAM_DATA` 以外の draft 15 の仕様参照は別途行う仕様参照の同期で更新する
