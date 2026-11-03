# draft 16 対応完了後に仕様参照とセッションエラーの説明を同期する

- Created: 2026-11-03
- Completed: {YYYY-MM-DD}
- Branch: feature/update-wt-h2-draft16-references
- Polished: 2026-11-03

## 目的

draft 16 の動作変更への対応が終わった後、コードコメント・利用者向け文書・テストの仕様参照を実装と一致させる。特に WebTransport ストリームの状態違反がセッション全体のエラーであることを明確にする。分類はドキュメント更新とし、プロトコル動作の変更は別 issue で扱う。

## 現状

- `refs/` は HTTP/2 draft 16、HTTP/3 draft 17、overview draft 14 へ更新済みだが、実装は HTTP/2 draft 15 に基づいている。
- `src/webtransport.rs` の `WtSession::handle_stream_data` などには、`WT_STREAM_STATE_ERROR` を「ストリームエラー」とする記述がある。draft 16 は Section 6.2・6.3・6.4・6.6・6.9 の該当文を「セッションエラー」に変更している。
- 実際の `crates/tokio-http2/src/webtransport.rs` の `DriverState::abort_session_with_wt_error` と `wt_http2_error_code` は、既に CONNECT ストリームへ対応する RST_STREAM を送り、セッションを終了する。WebTransport の個別ストリームだけをリセットする動作へ変更してはならない。
- `CODEBASE.md`、`src/webtransport/`、`src/settings.rs`、`src/limits.rs`、`src/error.rs`、`crates/tokio-http2/`、`examples/wt_server/`、`skills/shiguredo-http2/SKILL.md`、`tests/`、`pbt/` に draft 15 の説明が残る。

## 差分確認結果と担当範囲

HTTP/2 draft 15 → 16 は本文の変更を節ごとに比較し、ページ送り・目次・索引・日付・改行の変化と区別した。

| 変更箇所 | 内容 | 対応先・判断 |
| --- | --- | --- |
| Section 3.4 | セッション終了・アプリケーションの終了通知・CONNECT の reset の説明を整理 | 既存の終了経路の説明を同期する。WT_CLOSE_SESSION によるアプリケーション通知と、HTTP/2 エラーによるセッション終了を区別する |
| Section 4.3.2 | Structured Fields の規範参照を RFC 9651 に変更 | 0168 でパーサーを対応させ、本 issue で残る参照を確認する |
| Section 4.3.2 | ヘッダーはストリームデータ初期上限のみを持ち、セッション上限・ストリーム数は開始時の capsule でも提供できることを明記 | `WtInit` のキーを追加しない。既存の `u`・`bl`・`br` の説明を確認する |
| Section 4.4 | 中継が広告する上限にも厳密な増加を要求 | 本ライブラリに中継機能はない。送出側の規則は 0165〜0167 に含める |
| Section 5.2 | ストリーム ID の下位 2 bit と 4 種類の表を明記 | `src/webtransport/stream.rs` の `stream_id` は一致している。動作変更は不要 |
| Section 6.1 | 非ゼロ PADDING の任意のエラー処理を「セッションエラー」に修正 | `WtSession::handle_capsule` は PADDING を無視しており、許可されている動作を維持する |
| Section 6.2・6.3・6.4・6.6・6.9 | 状態違反を「セッションエラー」と明記 | 既存のエラーマッピングを維持し、誤解を招くコメントを修正する。E2E の不足は既存 0140 の範囲であり重複起票しない |
| Section 6.4 | WT_STREAM がストリーム数・セッションデータ・ストリームデータの全上限に従うことを明記 | `create_peer_streams_up_to`、`WtStream::recv_data`、`WtFlowControl::consume_recv` に検証があり、今回の差分による新規処理は不要 |
| Section 6.5・6.6・6.7 | 初期値および直前の値と同じ上限の受信も禁止 | 0165・0166・0167 で対応する |
| Section 4.2、6.5、6.7、6.8、6.10、6.12、6.13 | HTTP/3 文書内の参照節を修正 | 該当する参照コメントを照合する。本文で指定された版と、ローカルの最新版の節を混同しない |
| Section 8 | サーバーの SETTINGS_WT_ENABLED を例に追加し、Origin を HTTPS origin に修正。初期フロー制御の省略を注記 | ローカルの説明・例に同種の誤記が残っていないか確認する。新しい要件として扱わない |
| Section 9.1、11.2 | 非互換な draft の識別方針を追加し、WT_ENABLED を 0x2b67 に変更 | 0164 で対応する |
| Section 11.4 | IANA の登録項目名を Reference に変更し、WT_STREAM_DATA_BLOCKED の登録記載を追加 | `src/webtransport/capsule.rs` の既存値 0x190B4D42 と一致している。capsule の追加や値変更は不要 |
| Section 12 | RFC 9651 への移行、overview・HTTP/3・partial reset の参照版を更新 | 原本を編集せず、コードが根拠とする資料と節を確認する |

同時に更新した関連 draft についても全文差分を確認した。

| 文書 | 変更内容 | 本リポジトリでの判断 |
| --- | --- | --- |
| HTTP/3 draft 16 → 17 | サーバーの HTTPS URI 識別、クライアント SETTINGS と draft 交渉、0-RTT 時の設定破棄、WT_STREAM の配置とエラー種別、RESET_STREAM_AT の Reliable Size と中継、確立前ストリームの拒否、接続共有時のフロー制御によるデッドロック、WT_ENABLED 識別子を更新。初回 MAX_DATA・MAX_STREAMS の比較対象も明確化。重複文の削除・参照版更新などの編集変更もある | HTTP/3・QUIC 固有の変更は `CODEBASE.md` により対象外。共通の上限の規則は HTTP/2 自身の Section 6.5・6.7 に基づき 0165・0167 で対応する。HTTP/2 の Reliable Size の規則を HTTP/3 固有の規則へ置き換えない |
| overview draft 13 → 14 | セッション作成時に渡す URI を requester ではなく server endpoint の URI と明確化。文書の日付・CSP の参照日付を更新 | 既存のサーバー URI に対する CONNECT と一致し、動作変更は不要 |

追加した `refs/rfc9846.txt` は、Section 1.2 で RFC 8446 に対する後方互換な TLS 1.3 の改訂と位置付けている。変更一覧には KeyShare の再利用禁止、旧 TLS の禁止、PSK・HelloRetryRequest のハッシュの明確化、NewSessionTicket の扱い、鍵更新の要件・回数制限、alert・構造体の境界値の修正、KEM を含む鍵交換の表現、transcript hash・RSA PSS 要件の整理がある。TLS の内部処理は `crates/tokio-http2/src/tls.rs` が利用する rustls の責務であり、本 issue で再実装しない。直接利用する Section 7.5 の Exporter はインターフェースと計算式が維持され、`DriverState` は `EXPORTER-WebTransport` と `Some(&ctx)` を渡している。WebTransport のコンテキスト構造の変更はないため、その変更 issue は起票しない。rustls 内部の RFC 9846 全体への適合性を確認済みとするものではない。

## 設計方針

- 0164〜0168 の動作変更が完了した後に実施する。番号だけを先に置き換えて draft 16 対応済みと表記しない。
- 現行の説明に残る draft 15 の参照を検索し、根拠と動作を確認して draft 16 に同期する。`WtInit` の RFC 8941 参照の残存も確認する。別ヘッダーも規範参照を個別に照合する。`WT-Available-Protocols` / `WT-Protocol` の根拠は、HTTP/2 draft 16 が参照する HTTP/3 draft 16 Section 3.3 と RFC 9651 であり、既存コメントが RFC 8941 であることを保全理由にしない。規範参照と現行実装の対応範囲を区別して説明し、パーサーの動作拡張・統合は本 issue に混在させない。
- `WT_STREAM_STATE_ERROR` の名前自体や公開 enum を変更せず、個別の WT ストリームの状態違反とセッションエラーの範囲を文章で区別する。
- `CODEBASE.md` の変更をコミットする場合は、同ファイルの Git 特別規約に従い、実装や通常の文書更新と分ける。
- 過去の `issues/closed/` と変更履歴は履歴として保全する。現行 `CODEBASE.md` の指示に従い `CHANGES.md` は変更しない。RFC 8446 など旧版の資料への参照も、単なる廃止を理由に機械的に書き換えない。
- 既存 0118 のバージョン番号・ビルド方式・モジュールパスの修正は扱わない。今回の仕様版・仕様の説明の同期に限定する。

## 完了条件

- 0164〜0168 の実装完了を確認したうえで、利用者向けの対応版、コメント、テストの根拠が draft 16 の動作と一致すること。
- 状態違反に対する説明が、CONNECT ストリームを終了するセッションエラーであることを正しく表していること。
- 差分表の各項目が、対応完了または動作変更不要と判断した根拠を維持していること。
- 削除された旧 draft ファイルへの現行のリンクを残さず、歴史的な説明と原本は保全すること。
- 動作や公開 API の変更を混在させず、`cargo fmt --all -- --check` とドキュメントテストが通ること。
