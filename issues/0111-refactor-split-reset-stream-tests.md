# reset_stream テストモジュールを分割する

- Created: 2026-08-10
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-split-reset-stream-tests
- Polished: 2026-08-28

## 目的

`tests/test_connection.rs` の `mod reset_stream` が約 4034 行に肥大し、可読性と保守性が低下している。テストターゲットをディレクトリモジュール化し、`mod reset_stream` を関心ごとのサブモジュールへ分割する。テスト本文は一切変更しない。

## 現状

以下は 2026-08-28 時点 (0107 適用前) の実測値である。

- `tests/test_connection.rs` は計 4695 行
- `mod reset_stream` は宣言位置からファイル末尾までで約 4034 行
- `cargo test --test test_connection` は 97 件が通過し、うち 76 件が `mod reset_stream` 配下、21 件がクレート直下
- ヘルパーはクレート直下に 4 個 (`encode_frame` / `create_continuation` / `encode_valid_request_headers` / `request_headers`)、`mod reset_stream` 内に 35 個 (`setup_server` / `setup_client` / `assert_headers_reset_events` / `assert_internal_reset` / `assert_delayed_data_discarded` 等) 定義され、テスト本体と混在している

`mod reset_stream` には以下の関心が混在している。

- ヘッダー経路のエラー処理テスト (`process_headers` の検証エラー・状態遷移エラー・状態遷移後 malformed)
- DATA 経路のエラー処理テスト (`handle_data` の Content-Length 不一致・no-content 違反・状態遷移違反)
- フロー制御違反によるリセットテスト
- 接続ウィンドウの枯渇・補充テスト
- 空 DATA・padding の破棄と消費量通知テスト
- 同時ストリーム数上限超過のリセットテスト (REFUSED_STREAM と GOAWAY の last-stream-id、HPACK 状態維持)
- CONNECT 確立済みストリームへの HEADERS・未知フレームのリセットテスト
- 明示的リセット・ピア由来の RST_STREAM・遅延フレーム破棄テスト

なお `src/connection.rs` (2165 行) と `src/connection/headers.rs` (970 行) は既にディレクトリモジュール化済みで、shiguredo-rust スキルの「テストが長くなるのはモジュール自体が大きすぎるサインなので `src/<module>.rs` 側の分割を検討すること」が指す本体側の分割検討は、本 issue の目的ではない。`issues/closed/0015-refactor-split-connection-module.md` は `src/connection/` へ `headers.rs` / `data.rs` / `settings.rs` を作成したが、`data.rs` / `settings.rs` は `mod` 宣言されずコンパイル対象外の重複実装のまま残り、`issues/closed/0051-bug-fix-connection-dead-code.md` で両ファイルが削除された (実質 `headers` のみの分割として確定している)。この経緯どおり本体側の再分割は公開 API とは別の設計判断を伴うため、本 issue は `src/` を変更しない。

## 設計方針

### 対応順

0107 が先。0107 は `tests/test_connection.rs` の `assert_delayed_data_discarded` の doc コメント更新と、`handle_continuation` の遅延破棄を検証するテストの新規追加を完了条件としており、いずれも本 issue が移動するファイル・ヘルパーに触れる。0111 が先になると 0107 の変更対象の記述が実位置と乖離し、同時進行では大きなコンフリクトになる。

### テストターゲットのディレクトリモジュール化

`tests/test_connection.rs` を `tests/test_connection/main.rs` へ移行し、クレート直下のテストを `tests/test_connection/` 配下へ展開する。構成は既存の `tests/test_hpack/` / `tests/test_stream/` / `tests/test_webtransport/` (いずれも `main.rs` は `mod` 宣言のみで `#[test]` を含まないことを実測済み) と、トップレベル由来のテストを `root.rs` にまとめる `issues/closed/0036-refactor-move-mod-tests-to-tests-dir.md` の前例に従う。

```
tests/test_connection/
├── main.rs                      # mod 宣言 + クレート直下ヘルパー 4 個 (#[test] を置かない)
├── root.rs                      # 現行のクレート直下テスト 21 件
├── reset_stream.rs              # mod 宣言 + use super::* + reset_stream 系の共有ヘルパー
└── reset_stream/
    ├── rst_stream.rs
    ├── delayed_frames.rs
    ├── headers_error.rs
    ├── data_error.rs
    ├── flow_control_violation.rs
    ├── connection_window.rs
    ├── discarded_data.rs
    ├── concurrent_limit.rs
    └── connect_established.rs
```

- `tests/test_connection.rs` と `tests/test_connection/main.rs` は Cargo のターゲット名が衝突するため同時存在できない。先に `mkdir -p tests/test_connection` してから `git mv tests/test_connection.rs tests/test_connection/main.rs` を行い、旧パスに残さない
- `reset_stream.rs` が 9 個のサブモジュールを `mod` 宣言し、各ファイルは `tests/test_connection/reset_stream/<名前>.rs` に置く (`mod.rs` は作らない)
- `reset_stream.rs` は `use super::*` を維持する。子孫モジュールはこれを経由してクレート直下のヘルパーを参照する
- `Cargo.toml` には `[[test]]` を宣言していないため、ターゲットは自動検出に任せる。`.github/workflows/ci.yml` と `Makefile` は `cargo test --workspace` のみを実行してターゲット名を列挙しておらず、`crates/` 配下にも `[[test]]` 宣言が無いため、構成変更による追加編集は不要
- `root.rs` へ移す 21 件は対象を絞った追加分割ではなく、`main.rs` に `#[test]` を置かない前例に合わせて移動するだけである。関心別の再編は行わない
- 上のツリー内の各サブモジュールの主題は以下のとおり。ただし**帰属の判定には使わない**（次の節の確定表のみが基準）
  - `rst_stream`: 明示的リセット・ピア由来の RST_STREAM・idle / 接続 / 偶数 ID へのリセット・二重リセット・GOAWAY の last-stream-id・リセット由来の内部イベント通知
  - `delayed_frames`: クローズ済みまたはリセット済みストリームへの遅延フレーム (DATA / HEADERS / WINDOW_UPDATE / RST_STREAM) の破棄
  - `headers_error`: ヘッダー受信経路 (`handle_headers` / `handle_continuation` / `process_headers`) の検証エラー・状態遷移エラー・状態遷移後 malformed
  - `data_error`: DATA 経路 (`handle_data`) の Content-Length 不一致・no-content 違反・状態遷移違反
  - `flow_control_violation`: ストリーム / 接続ウィンドウ超過によるフロー制御違反のリセット
  - `connection_window`: 接続ウィンドウの枯渇・補充とアプリによる補充の検証
  - `discarded_data`: 空 DATA・padding の破棄と `Event::DataDiscarded` の消費量通知
  - `concurrent_limit`: 同時ストリーム数上限超過 (REFUSED_STREAM) と GOAWAY・HPACK 状態維持
  - `connect_established`: CONNECT 確立済みストリームへの HEADERS・未知フレーム

### 76 件の帰属先 (確定表)

受け皿は上記 9 個に確定する (例示や「等」で終わらせない)。1 つのテストが複数の主題に該当し得るため、**下表の帰属のみを基準**とし、主題説明での判断は行わない。

| サブモジュール | 件数 | 帰属するテスト |
|---|---|---|
| `rst_stream.rs` | 12 | `test_explicit_reset_stream_pushes_event` / `test_peer_rst_stream_pushes_event` / `test_reset_stream_closed_no_event` / `test_reset_stream_on_idle_stream_is_error` / `test_reset_stream_on_even_stream_id_is_error` / `test_reset_stream_twice_sends_rst` / `test_reset_stream_on_connection_id_is_error` / `test_reset_stream_implicitly_closed_stream_sends_rst` / `test_reset_stream_send_buffer_not_flushed` / `test_reset_stream_included_in_goaway_last_stream_id` / `test_reset_new_stream_included_in_goaway_last_stream_id` / `test_decode_error_pushes_stream_reset` |
| `delayed_frames.rs` | 7 | `test_reset_stream_delayed_data_discarded` / `test_reset_stream_delayed_headers_after_goaway` / `test_reset_stream_delayed_window_update_received` / `test_reset_stream_delayed_rst_stream_ignored` / `test_reset_stream_decode_error_keeps_connection` / `test_new_stream_headers_after_goaway_is_error` / `test_stream_error_reset_delayed_data_discarded` |
| `headers_error.rs` | 25 | `test_malformed_1xx_end_stream_resets_stream` / `test_malformed_content_length_end_stream_resets_stream_server` / `test_malformed_content_length_end_stream_resets_stream_client` / `test_malformed_content_length_open_state_resets_stream` / `test_malformed_content_length_end_stream_resets_stream_continuation` / `test_no_content_content_length_end_stream_headers_accepted` / `test_content_length_zero_end_stream_headers_accepted` / `test_initial_headers_without_pseudo_resets_stream` / `test_initial_response_without_pseudo_resets_stream` / `test_new_stream_validation_error_via_continuation_resets_stream` / `test_pseudo_headers_in_non_initial_headers_resets_stream` / `test_pseudo_headers_in_non_initial_headers_resets_stream_client` / `test_trailer_validation_error_resets_stream` / `test_trailer_validation_error_resets_stream_client` / `test_trailer_without_end_stream_resets_stream` / `test_trailer_without_end_stream_resets_stream_client` / `test_request_headers_missing_method_resets_stream` / `test_response_headers_status_101_resets_stream` / `test_protocol_without_enable_connect_protocol_resets_stream` / `test_invalid_content_length_resets_stream_server` / `test_invalid_content_length_resets_stream_client` / `test_signed_content_length_resets_stream_server` / `test_signed_content_length_resets_stream_client` / `test_headers_on_half_closed_remote_resets_stream` / `test_headers_on_half_closed_remote_resets_stream_client` |
| `data_error.rs` | 10 | `test_content_length_exceeded_pushes_stream_reset` / `test_content_length_mismatch_on_end_stream_pushes_stream_reset` / `test_content_length_exact_match_accepts_data` / `test_no_content_violation_pushes_stream_reset` / `test_no_content_empty_data_with_end_stream_accepted` / `test_no_content_empty_data_without_end_stream_accepted` / `test_no_content_head_with_content_length_empty_data_accepted` / `test_no_content_padding_only_data_accepted` / `test_no_content_padding_only_data_with_end_stream_closes_stream` / `test_data_on_half_closed_remote_pushes_stream_reset` |
| `flow_control_violation.rs` | 2 | `test_flow_control_violation_pushes_stream_reset` / `test_window_overflow_pushes_stream_reset` |
| `connection_window.rs` | 2 | `test_connection_window_exhaustion_without_replenishment` / `test_connection_window_replenishment_keeps_connection` |
| `discarded_data.rs` | 6 | `test_empty_data_discarded_no_event` / `test_padded_data_discarded_counts_padding` / `test_padded_violation_data_counts_padding_in_stream_reset` / `test_padding_only_data_discarded_notifies_consumed` / `test_data_discarded_after_normal_close` / `test_stream_error_then_delayed_data_reports_discarded` |
| `concurrent_limit.rs` | 7 | `test_concurrent_stream_limit_exceeded_resets_stream` / `test_concurrent_stream_limit_exceeded_via_continuation_resets_stream` / `test_concurrent_stream_limit_exceeded_included_in_goaway_last_stream_id` / `test_concurrent_stream_limit_exceeded_keeps_hpack_state` / `test_concurrent_stream_limit_exceeded_keeps_hpack_state_via_continuation` / `test_concurrent_stream_limit_zero_resets_stream` / `test_concurrent_stream_limit_exceeded_twice_keeps_connection` |
| `connect_established.rs` | 5 | `test_headers_on_established_connect_resets_stream` / `test_headers_on_established_connect_resets_stream_continuation` / `test_unknown_frame_on_established_connect_resets_stream` / `test_connect_established_headers_reset_keeps_hpack_state` / `test_connect_established_headers_reset_keeps_hpack_state_via_continuation` |

- 上表は 2026-08-28 時点の `mod reset_stream` 配下 76 件の全テストを漏れなく 1 回ずつ列挙している (件数の合計は 76)
- 0107 で追加される `handle_continuation` の遅延破棄を検証するテストは `delayed_frames.rs` に帰属させる
- 実装直前に `cargo test --test test_connection -- --list` で一覧を取得し、上表に存在しないテストが増えている場合は同じ基準 (主検証対象が遅延フレーム破棄か、内部イベント通知か、経路別の検証か) で受け皿を決め、上表へ追記して着手すること

### ヘルパーの配置

- `mod reset_stream` 内の 35 個のヘルパーは、2 つ以上のサブモジュールから使うものは `reset_stream.rs` に置き、単一サブモジュール専用はそのサブモジュールへ移す。テスト本体から直接呼ばれず他ヘルパー経由でしか使われないヘルパー (例: `assert_rst_stream_sent_with_code` は `assert_rst_stream_sent` / `assert_internal_reset` / `assert_headers_reset_events` から、`encode_connect_request_headers` は `establish_connect_tunnel` からのみ使われる) は、それを利用するヘルパーの帰属先に追随させる
- 可視性の変更は行わない。親モジュールの private な項目は Rust の可視性規則により子孫モジュールから見えるため、サブモジュールは `use super::*` だけで `reset_stream.rs` の共有ヘルパーとクレート直下ヘルパーの両方を参照できる (実測で確認済み)。`root.rs` も先頭に `use super::*` を置けばクレート直下ヘルパーを修飾なしで使える
- `tests/helpers/` は新設しない。ヘルパーはすべて `test_connection` ターゲット内でしか使われておらず (`setup_server` 等の利用は同ターゲット内に限定され、`pbt/` は別クレートで `pbt/tests/prop_connection/main.rs` に自前の `encode_frame` / `setup_client_server` を持つ)、shiguredo-rust スキルの「テスト間で共有するヘルパーは `tests/helpers/` に置くこと」が想定する状況 (複数のテストバイナリで共有するヘルパー) は本件では成立しない。`tests/helpers/` を作ってもどの Cargo ターゲットからも参照できず (同ディレクトリに `main.rs` を置くと空の `helpers` ターゲットが増える)、`#[path]` による前例のない取り込み機構の初導入になる
- 移動に伴う編集は import のみに限定する

### 内容の変更禁止

- テスト関数名、検証ロジック、アサーション、期待値、コメントは変更しない。移動と import 調整のみ行う
- テストの並び順は元の順序を維持する (レビュー時の差分追跡を容易にするため)

## 完了条件

- `tests/test_connection.rs` が存在しない (ディレクトリモジュールへ移行済み)
- `tests/test_connection/main.rs`、`tests/test_connection/root.rs`、`tests/test_connection/reset_stream.rs`、`tests/test_connection/reset_stream/` 配下の 9 サブモジュールが作成されている
- `main.rs` と `reset_stream.rs` に `#[test]` が無い
- 分割直前に実測した `cargo test --test test_connection` の passed 件数と、`cargo test --test test_connection -- --list` のテスト名集合が、分割後と一致する (モジュール経路の修飾を除いた関数名の集合が一致すること。2026-08-28 時点の実測は 97 件で、0107 適用後は追加テスト分だけ増加した値が基準になる。絶対値は基準としない)
- `mod reset_stream` 配下のテストが、確定表の帰属どおりに 9 サブモジュールへ配置されている (重複配置・表外配置が無い)
- テスト本文の追加・削除・書き換えが無いことが `git diff` で確認できる (移動と import の変更のみ)
- `src/` 配下に変更が無い
- `CHANGES.md` の `## develop` の `### misc` にテスト分割の `[UPDATE]` エントリと担当者行が追加されている (`issues/closed/0036-refactor-move-mod-tests-to-tests-dir.md` が `### misc` への `[UPDATE]` エントリと担当者行の実例)
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `tests/test_connection.rs` — `mod reset_stream`
- `src/connection.rs` — `Connection::handle_data` / `Connection::reset_stream_internal` / `Connection::try_remove_closed_stream`
- `src/connection/headers.rs` — `Connection::process_headers` / `Connection::handle_headers` / `Connection::handle_continuation`
- `issues/closed/0036-refactor-move-mod-tests-to-tests-dir.md` — `main.rs` に `mod` 宣言と共通ヘルパーを置き、トップレベル由来のテストを `root.rs` へまとめる構成と、`### misc` エントリの実例
- `issues/closed/0048-refactor-split-pbt-submodules.md` — テスト分割の完了条件 (移行前後の passed 件数一致) を求める文面の参考。ただし 0048 自体は「対応不要」として closed され、`### misc` エントリは `CHANGES.md` に未反映
- `issues/closed/0015-refactor-split-connection-module.md` / `issues/closed/0051-bug-fix-connection-dead-code.md` — `src/connection/` の分割と、`mod` 宣言されずデッドコードとなったサブモジュールの削除の経緯
- `issues/0107-change-remove-closed-stream-check.md` — 先行して対応する issue。同一ファイル・同一ヘルパーを変更する
- shiguredo-rust スキル — テスト (テストファイル分割・`tests/helpers/`・`mod.rs` 禁止)
