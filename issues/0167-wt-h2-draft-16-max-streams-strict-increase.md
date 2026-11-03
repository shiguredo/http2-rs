# WT_MAX_STREAMS を方向別の初期上限から厳密に増加させる

- Created: 2026-11-03
- Completed: {YYYY-MM-DD}
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
