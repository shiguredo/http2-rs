# WebTransport ストリーム ID 記録の追い出し既知制限を明文化する

- Created: 2026-11-09
- Completed: {YYYY-MM-DD}
- Branch: feature/update-wt-stream-id-record-limit
- Polished: 2026-11-09

## 目的

`STREAM_ID_RECORD_MAX_SIZE` (10000) による上限付き記録の見直しの結論をコードに残す。draft-ietf-webtrans-http2-16 Section 6.4 は有効でない状態のストリームへの WT_STREAM 受信に WT_STREAM_STATE_ERROR でセッションエラーを送ることを MUST とし、Section 6.6 は初期値または最後に受理した上限を増加させない WT_MAX_STREAM_DATA を WT_FLOW_CONTROL_ERROR で拒否することを MUST とする。また Section 6.3 は 2 回目の WT_STOP_SENDING 受信にも WT_STREAM_STATE_ERROR を MUST とする。上限付き追い出しは、記録が追い出し単位で 10000 件を超えた場合にこれらの検出を漏らす条件付きの制限であり、漏れは記録ごとに発生条件が異なる。追い出しの発生条件・漏れの内容・維持理由を実装に隣接した正確な文書として置き、漏れの挙動をテストで固定する。

## 現状

- `src/webtransport.rs` の `STREAM_ID_RECORD_MAX_SIZE = 10000` は `WtSession::closed_stream_send_max: BoundedMap<WtStreamId, u64>` と `WtSession::stop_sending_received_ids: BoundedSet<WtStreamId>` の 2 記録で共用する
- `closed_stream_send_max` の用途は次の 3 つ
  - 0128 (クローズ・リセット済みストリームへの WT_STREAM を WT_STREAM_STATE_ERROR で拒否する) の `contains` による判定 (`handle_stream_data` 冒頭)
  - 0152 (削除済み ID を記録済みとして暗黙再作成しない) の `contains` による判定 (`will_create_implicit_peer_bidi_stream` と `create_peer_streams_up_to`)
  - 0166 (ID の初期値または最後に受理した送信上限の値を記録し、削除後の非増加 WT_MAX_STREAM_DATA を検出する。削除後の増加は記録だけ更新して無視する) の `get` / `insert`
- `stop_sending_received_ids` は 0151 (削除済みストリーム後も、2 回目の WT_STOP_SENDING と STOP_SENDING 受信後の WT_MAX_STREAM_DATA を拒否する) のため、WT_STOP_SENDING を受理した時点でストリームの有無を問わず記録する
- `src/bounded_map.rs` / `src/bounded_set.rs` は要素数超過で最も小さいキーを追い出す。系統をまたぐと数値順と受理順が一致しない旨をモジュール doc に注記している
- 既知制限 (追い出しで検出を失うこと) の説明は分散している。定数 doc、`closed_stream_send_max` のフィールド doc (非増加検出の漏れ。削除済みローカル開始ストリームでは初期値との比較も失われる)、`handle_stream_data` 冒頭のコメント、`create_peer_streams_up_to` の doc、および `tests/test_webtransport/integration.rs` の既存テスト内の注記
- 上限超過の実際の挙動 (追い出された最古 ID への WT_STREAM が新規ストリームとして受理されること) を検証するテストは存在しない。追い出し自体のテストは `src/bounded_map.rs` / `src/bounded_set.rs` のコンテナ単体テストのみであり、記録の挙動としては未検証

## 見直しの結論

- 上限値 10000 は維持する。ストリーム ID は仕様上再利用されない (RFC 9000 Section 2.1) ため、前記 MUST の検出を完全に保証するには記録が無上限になり、すべてのセッションで常に有界である現行の方針 (BoundedMap の導入経緯 0166、クローズ済み記録に上限を持つ方針 0128 と 0053 に連なる) と両立しない。実害条件に入るのは 10000 件を超える追い出し単位 ( `closed_stream_send_max` は閉鎖数、`stop_sending_received_ids` は WT_STOP_SENDING の受理数) の場合だけである。この理由を定数 doc に書く
- 記録構成 (2 記録分離) も維持する。`closed_stream_send_max` は削除済みストリームの送信上限値の記録であり、`stop_sending_received_ids` はストリームの有無を問わず WT_STOP_SENDING を受理した事実の記録である。単一記録に統合すると生存ストリームの ID まで `handle_stream_data` の閉鎖判定に混ざり、生存ストリームの正当な WT_STREAM を誤って拒否し得るため安全域が失われる

## 設計方針

- 定数 doc を 1 本に集約して、次を正確に書き換える
  - 共用の理由 (2 記録とも同一上限で運用する)
  - 追い出し基準 (最も小さいキー。系統内では最古。系統をまたぐと受理順と一致しない)
  - 追い出し基準は記録ごとに書き分けること。`closed_stream_send_max` は閉鎖したストリームの記録が 10000 件を超えたとき、`stop_sending_received_ids` は WT_STOP_SENDING の受理が 10000 件を超えたときに最も小さい ID が追い出される。後者は生存ストリームへの受理でも記録が伸びるため、閉鎖数とは別の条件である
  - 追い出された ID で漏れる検出の列挙は記録の経路に紐付けること。`closed_stream_send_max` 由来: 追い出されたクローズ済み ID への WT_STREAM が新規ストリームとして受理され得る (0128)、削除済み ID への非増加 WT_MAX_STREAM_DATA が検出されず、上限値との比較も失われる (0166)。`stop_sending_received_ids` 由来: 削除済み ID の 2 回目の WT_STOP_SENDING が受理され得る (0151)。記録済み ID を暗黙再作成しない判定 (0152) は `closed_stream_send_max` を使うが、この判定の漏れは「追い出し済み ID に限って遅延 capsule が暗黙生成を許す既知挙動」ではなく 0128 と同じ漏れに含める (暗黙生成は受信 capsule 経路でしか起こらないため)
  - 各漏れが draft-ietf-webtrans-http2-16 Section 6.4 / Section 6.3 / Section 6.6 のどの MUST の検出に相当するかを述べること。ただし、記録に残る範囲を超えた遅延 capsule の検出まで保証する文言は書かない (原理的に待ち受けできないため)
  - 追い出し基準は最も小さいキーであり、スパンの外側 (追い出し境界より古い ID) に限り漏れること
- 分散箇所の整理
  - 定数 doc に集約した結果、重複する記述は縮める: `handle_stream_data` 冒頭 (記録から追い出された ID は対象外の注記)、`create_peer_streams_up_to` (追い出された ID は再作成され得る既知の制限)、`closed_stream_send_max` のフィールド doc (非増加検出の漏れと初期値比較の喪失)
  - 統合前後で内容を失わないこと。特に `closed_stream_send_max` フィールド doc にある「削除済みローカル開始ストリームでは初期値との比較も失われる」は定数 doc または当該フィールド doc に必ず残す
  - `create_implicit_peer_bidi_stream` / `will_create_implicit_peer_bidi_stream` の doc にある「記録済み ID を再作成しない (0152 方針)」の説明は追い出しの既知制限とは別の内容であり、残す
  - `stop_sending_received_ids` と `WtStream::stop_sending_received` の OR 判定 (述語 `stop_sending_received`) と、記録から追い出された ID は生存フラグでは代替できない旨の説明は `stop_sending_received_ids` の側に残す
- 追い出し後の挙動を固定するテストを `tests/test_webtransport/integration.rs` に追加する (公開 API での再現経路のみで成立するため既存方針に従う。private テストは作らない)
  - `src/webtransport.rs` には現行で `#[cfg(test)]` モジュールが存在しない点に注意する
  - ピア開始 bidi ストリームを 10001 件閉じて `closed_stream_send_max` から最古 ID を追い出す。閉じ方は既存の統合テストの経路に従い、ピアからの FIN 付き WT_STREAM capsule とローカル側の送信終端 (`send_stream_data` の fin または `reset_stream`) 双方で双方向ストリームを終端する必要がある (`WtStream::is_closed` は両端の終端状態のため)
  - 大量のピア開始ストリームには `WtConfig::initial_max_streams_bidi` (公開フィールド) を 10001 以上にした peer 用 config を渡して受信上限を担保する
  - 追い出された最古 ID への WT_STREAM が新規ストリームとして受理され `WtEvent::StreamOpened` が再送出されること (現行の方針どおりの既知制限であることを日本語コメントで明記。draft-ietf-webtrans-http2-16 Section 6.4 の MUST 検出の漏れを許す挙動である旨も書く)
  - 追い出されていない (記録に残る) 2 番目に古い ID への WT_STREAM は現行どおり WT_STREAM_STATE_ERROR で拒否されること (境界確認)
- 動作は一切変更しない。`WtSession` の判定構造と上限値はそのまま

## スコープ外

- 上限値の変更 (10000 のまま)
- `BoundedMap` / `BoundedSet` の追い出し方針・記録構成の変更
- HTTP/2 側 (`src/connection.rs` の `CLOSED_STREAMS_MAX_SIZE` と `closed_streams`) の上限と doc の変更
- `tests/test_webtransport/integration.rs` を関心ごとに分割する作業 (0156-refactor-split-webtransport-integration-tests.md が扱っている)

## 完了条件

- `STREAM_ID_RECORD_MAX_SIZE` の doc が、共用理由・追い出し基準 (記録ごとの条件)・漏れる検出の列挙 (記録の経路に紐付け) を 1 本で述べる
- 分散していた既知制限の注記のうち、重複を縮めてよい箇所 (`handle_stream_data` 冒頭、`create_peer_streams_up_to`、`closed_stream_send_max` のフィールド doc) が定数 doc への集約と整合する。0152 の非再作成方針の説明 (`create_implicit_peer_bidi_stream` / `will_create_implicit_peer_bidi_stream`) と、`stop_sending_received` の OR 判定とその既知制限の説明 (`stop_sending_received_ids`) は残す
- 追い出し既知制限を固定するテストが `tests/test_webtransport/integration.rs` に追加され、許可側 (最古は受理) と拒否側 (記録に残る ID は拒否) の双方が検証されている
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `src/webtransport.rs` — `STREAM_ID_RECORD_MAX_SIZE` / `WtSession::closed_stream_send_max` / `WtSession::stop_sending_received_ids` / `WtSession::handle_stream_data` / `WtSession::will_create_implicit_peer_bidi_stream` / `WtSession::create_peer_streams_up_to` / `WtSession::create_implicit_peer_bidi_stream` / `WtSession::stop_sending_received` / `WtSession::remove_if_closed`
- `src/bounded_map.rs` / `src/bounded_set.rs` — 追い出し基準とモジュール doc
- `src/webtransport/stream.rs` — `WtStream::send_max` (記録される値) / `WtStream::is_closed` (双方向ストリームの閉鎖条件)
- `src/webtransport/flow_control.rs` — `can_accept_stream` (受信ストリーム数上限の判定)
- `src/connection.rs` — `CLOSED_STREAMS_MAX_SIZE` / `closed_streams` (スコープ外の HTTP/2 側記録)
- `tests/test_webtransport/integration.rs` — クローズ済みストリームへの WT_STREAM 拒否の既存テスト群と注記
- `refs/draft-ietf-webtrans-http2-16.txt` — Section 3.4 / Section 6.3 / Section 6.4 / Section 6.6
- `refs/rfc9000.txt` — Section 2.1 (ストリーム ID の再利用禁止)
- `issues/closed/0128-bug-wt-closed-stream-recreated.md` — WT_STREAM 拒否と記録済み ID 再作成抑止
- `issues/closed/0151-bug-closed-stream-stop-sending-state.md` — WT_STOP_SENDING 受理済み ID の記録と重複・順序検証
- `issues/closed/0152-bug-peer-bidi-implicit-stream-creation.md` — 記録済み ID の暗黙再作成抑止
- `issues/closed/0166-wt-h2-draft-16-max-stream-data-strict-increase.md` — `BoundedMap` 導入と削除後の非増加検出 (追い出し既知制限の doc の由来)
