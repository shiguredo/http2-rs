# WT_MAX_STREAMS を方向別の初期上限から厳密に増加させる

- Created: 2026-11-03
- Completed: 2026-11-03
- Branch: feature/change-wt-max-streams-strict-increase
- Polished: 2026-11-03

## 目的

WebTransport over HTTP/2 draft 16 Section 6.7 に従い、双方向・単方向それぞれの累積ストリーム数上限を増加させない `WT_MAX_STREAMS` の送受信を拒否する。分類は後方互換のない仕様変更とする。

## 仕様差分

draft 15 は直前の広告値より小さい `Maximum Streams` を受信した場合にセッションを終了する規則だった。draft 16 は同値も `WT_FLOW_CONTROL_ERROR` のセッションエラーとする MUST に変更した。対象方向の capsule を初めて受信するときも、Section 4.3 の対応する初期値を超える必要がある。

`Maximum Streams` が 2^60 以下であること、閉じたストリームも含む累積上限であること、bidi と uni が別々の上限を持つことは変わっていない。

一次資料: [draft-ietf-webtrans-http2-16 Section 6.7](https://datatracker.ietf.org/doc/html/draft-ietf-webtrans-http2-16#section-6.7)

## 現状

- `src/webtransport/flow_control.rs` の `WtFlowControl::update_max_streams` は、bidi・uni とも現在値より小さい場合だけ拒否する。両方向の初期上限を 2 にして `update_max_streams(2, true/false)` を実行すると、どちらも `Ok(())` になることを確認した。
- `src/webtransport.rs` の `WtSession::send_max_streams` は 2^60 上限のみ検査し、同値・減少を広告できる。初期値と同値の送信が成功することを確認した。直接広告した値はローカルのストリーム数上限へ反映されない。
- `grow_max_streams` は先に `add_max_streams_local` でローカル状態を更新してから送信する。この順序のまま現在値との `<=` 比較を追加すると、正常な増加も拒否してしまう。

## 設計方針

- 受信時は、対象方向の初期値または最後に受理した上限を超える値だけを反映する。同値・減少は `WtErrorKind::FlowControlError` とし、他方の方向へ影響させない。
- 送信時も対象方向のローカル広告値を超える値だけを出力し、成功した広告と `can_accept_stream` の判定に用いる上限を一致させる。
- `grow_max_streams` と直接広告の経路を揃え、検証してから一度だけ状態を更新する。増分 0、2^60 超過、加算による範囲超過を出力・状態更新前に処理する。
- `WebTransport-Init` にセッション上限やストリーム数のキーを追加しない。Section 4.3.2 の追加説明どおり、初期値は SETTINGS、開始直後の追加枠は capsule で扱う。

## 完了条件

- 初期値 0・非 0、および更新後の同値・減少・増加を bidi と uni で別々に検証すること。
- 一方の方向の更新・拒否が他方の上限や開設数を変更しないこと。
- 2^60 への正常な増加を受理し、到達後の同値・超過を拒否すること。
- 送信 API の拒否時に出力・上限を変更せず、正常な広告後は新上限までのストリームを受け入れられること。
- `tests/test_webtransport/flow_control.rs`、`tests/test_webtransport/integration.rs` と関連 PBT で検証し、`cargo test -p shiguredo_http2` と `cargo test -p pbt` が通ること。

## 解決方法

- `WtFlowControl::update_max_streams` を、対象方向の初期値または最後に受理した上限を増加させない値と 2^60 超過で `flow_control_error` を返すように変更した
- `WtFlowControl::add_max_streams_local` を削除し、ローカル上限を広告値へ進める `WtFlowControl::update_max_streams_local` を追加した。2^60 超過・非増加は拒否し、成功時だけ `can_accept_stream` が使うローカル上限を更新する
- 2^60 と増加の検証を private helper に集約し、受信・送信で同じ規則を使うようにした
- `WtSession::send_max_streams` はセッション状態 (`Active` / `Draining`) を検証したうえで `update_max_streams_local` に検証とローカル上限の更新を委譲し、成功時に広告値とローカル上限を一致させるようにした。拒否時は出力と上限を変更しない
- `WtSession::grow_max_streams` は事前のローカル上限更新をやめ、現在のローカル上限に増分を加えた値を `send_max_streams` に委譲するようにした。増分 0、u64 の範囲超過、2^60 超過は拒否する
- `WtConfig::apply_advertised_settings` を追加し、tokio の `WtServerRequest::accept` が未広告の WT 初期値を 0 に正規化してから SETTINGS の広告値を適用するようにした。ローカル設定と SETTINGS の広告値を一致させ、厳密増加検証の基準を正しくする
- `WtFlowControl` に `max_streams_bidi_remote` / `max_streams_uni_remote` を公開し、方向別の状態を観測できるようにした
- `fuzz/fuzz_targets/fuzz_wt_flow_control.rs` のアクションを削除した `add_max_streams_local` から `update_max_streams_local` に追従させた
- `tests/test_webtransport/flow_control.rs` に同値・減少の拒否、方向の独立性、ローカル広告 API の 2^60 と厳密増加を追加した
- `tests/test_webtransport/integration.rs` に送信の同値・減少拒否、増分 0、bidi / uni の 2^60 境界、u64 オーバーフロー、受信 capsule の同値エラー、広告後の新上限までのストリーム受け入れ、状態ガードを追加した
- `tests/test_webtransport/root.rs` に送信 API の 2^60 境界と同値拒否、`apply_advertised_settings` の未広告 0 化を追加した
- `pbt/tests/prop_webtransport/flow_control.rs` に `WT_MAX_STREAMS` の受信・広告・増分列の PBT を追加し、受理・拒否・方向の到達ゲートを設けた
- `crates/tokio-http2/tests/test_webtransport.rs` に WT_MAX_STREAMS の自動拡張とサーバー側の受理を検証する E2E テストを追加した
- `skills/shiguredo-http2/SKILL.md` の `send_max_streams` / `grow_max_streams` / `WtConfig` の説明と draft の適用範囲を更新した
- `CODEBASE.md` の指示に従い `CHANGES.md` は変更していない。`WT_MAX_STREAMS` 以外の draft 15 の仕様参照は別途行う仕様参照の同期で更新する
