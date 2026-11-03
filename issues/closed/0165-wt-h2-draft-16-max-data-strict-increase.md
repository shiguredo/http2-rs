# WT_MAX_DATA を初期値から厳密に増加する更新だけに制限する

- Created: 2026-11-03
- Completed: 2026-11-03
- Branch: feature/change-wt-max-data-strict-increase
- Polished: 2026-11-03

## 目的

WebTransport over HTTP/2 draft 16 Section 6.5 に従い、同じセッションデータ上限の広告を拒否する。受信の検証と送信の生成を同じ規則に合わせ、適合するピアにセッションを終了される capsule を生成しないようにする。分類は後方互換のない仕様変更とする。

## 仕様差分

draft 15 は直前の広告値より小さい `Maximum Data` を受信した場合にセッションを終了する規則だった。`refs/draft-ietf-webtrans-http2-16.txt` Section 6.5 は、上限を増加させない値をすべて `WT_FLOW_CONTROL_ERROR` のセッションエラーとする MUST に変更した。同値も対象となり、最初の capsule も Section 4.3 で確定した初期値を超える必要がある。

Section 4.4 の中継時の説明にも、送出する capsule がその接続の上限を増加させるという条件が追加された。

一次資料: [draft-ietf-webtrans-http2-16 Section 6.5](https://datatracker.ietf.org/doc/html/draft-ietf-webtrans-http2-16#section-6.5)

## 現状

- `src/webtransport/flow_control.rs` の `WtFlowControl::update_send_max` は `<` でのみ拒否する。`WtFlowControl::new(10, 10, 2, 2, 2, 2)` に `update_send_max(10)` を実行すると `Ok(())` になることを確認した。
- `src/webtransport.rs` の `WtSession::send_max_data` は varint 上限のみ検査し、現在の広告値以下の capsule も送出する。初期値と同値の送信が成功することを確認した。
- `send_max_data` は送信した上限を `WtFlowControl::recv_max` に反映していない。一方、`grow_recv_window` は先に `add_recv_max` で状態を更新してから `send_max_data` を呼ぶ。この順序のまま単純に `<= recv_max` の検査を追加すると、正常な自動拡張も拒否してしまう。

## 設計方針

- 受信した上限は、ピアの初期値または最後に受理した上限より大きい場合のみ反映する。同値・減少では `WtErrorKind::FlowControlError` を返し、上限を変更しない。
- `send_max_data` はローカルが広告済みの初期値・更新値を基準に、同値・減少を出力前に拒否する。成功時には広告値と受信可能量を一致させる。
- `grow_recv_window` と直接の `send_max_data` で更新経路を揃え、検証前の状態更新や二重加算を避ける。増分 0 や varint 上限到達後の非増加更新で capsule を生成しない。
- `crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_session_window` からの正の自動拡張が継続することを確認する。
- `crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_init_header_value_below_default_applied` は、クライアントが `SETTINGS_WT_INITIAL_MAX_DATA` を広告しておらず、セッションデータ上限の初期値は 0 である。クライアント側 `WtSession` の `initial_max_data` も 0 に揃え、`send_max_data(1 << 20)` で初めて上限を増加させる構成に変更する。これにより、既存の正常系が同値広告として拒否されるのを避ける。
- Sans I/O の受信エラーは `WtSession::process` から返す。既存の tokio ドライバの `abort_session_with_wt_error` により CONNECT ストリームを終了する経路を利用する。

## 完了条件

- 初回および更新後の同値・減少を拒否し、増加だけを受理すること。初期値 0 に対する最初の値 0 も拒否すること。
- 送信 API の拒否時に出力・受信上限を変更せず、正常な広告後はピアが新上限まで送ったデータを受信できること。
- `tests/test_webtransport/flow_control.rs`、`tests/test_webtransport/integration.rs` と関連 PBT で境界値と更新列を検証すること。
- 実際の capsule を入力した受信エラーと、tokio ドライバが `RST_STREAM(WT_FLOW_CONTROL_ERROR)` でセッションを終了する経路を検証すること。
- `cargo test -p shiguredo_http2`、`cargo test -p pbt` と関連する tokio の WebTransport テストが通ること。

## 解決方法

- `WtFlowControl::update_send_max` を、ピアの初期値または最後に受理した上限を増加させない値と varint 上限超過で `flow_control_error` を返すように変更した
- `WtFlowControl::add_recv_max` を削除し、広告値を絶対値で更新する `WtFlowControl::update_recv_max` を追加した (varint 上限超過・非増加を拒否し、旧 API からの移行方法を rustdoc に記載)
- `WtSession::send_max_data` は `update_recv_max` に検証と受信上限の更新を委譲し、成功時に広告値と受信上限を一致させるようにした
- `WtSession::grow_recv_window` は事前の状態更新をやめ、現在の受信上限に増分を加えた値を `send_max_data` へ委譲するようにした。増分 0 と varint 上限超過は拒否する
- `tests/test_webtransport/flow_control.rs` に送信上限・受信上限の同値/減少/varint 上限の検証を追加・更新した
- `tests/test_webtransport/integration.rs` に送信拒否時の状態/出力不変、広告後の上限までのデータ受信と超過拒否、`grow_recv_window` の増分 0・varint 境界 (MAX_VALUE - 1 からの到達と超過)・直接広告後の拡張、capsule 受信の同値/減少エラーを追加した
- `pbt/tests/prop_webtransport/flow_control.rs` を新設し、`WT_MAX_DATA` の更新列・広告列・`grow_recv_window` の増分列の PBT を追加した
- `crates/tokio-http2/tests/test_webtransport.rs` に tokio ドライバの自動拡張 (3 サイクルの継続) と、非増加の `WT_MAX_DATA` で `RST_STREAM(WT_FLOW_CONTROL_ERROR)` を返す E2E テストを追加した
- `fuzz/fuzz_targets/fuzz_wt_flow_control.rs` を削除した `add_recv_max` から `update_recv_max` に追従させ、CI の fuzz クレートチェックを通るようにした
- `skills/shiguredo-http2/SKILL.md` の `send_max_data` / `grow_recv_window` の説明を新しい契約に更新した
- `CODEBASE.md` の指示に従い `CHANGES.md` は変更していない。`WT_MAX_DATA` 以外の draft 15 の仕様参照は別途行う仕様参照の同期で更新する
