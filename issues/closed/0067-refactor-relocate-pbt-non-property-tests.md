# pbt/ に紛れ込んだ非該当テストを削除し、必要な分だけ tests/ に移植する

- Priority: Medium
- Created: 2026-06-10
- Completed: 2026-06-10
- Model: Opus 4.7
- Branch: feature/refactor-relocate-pbt-non-property-tests
- Polished: 2026-06-10

## 目的

`/review-code` (2026-06-10 実施) で「shiguredo-rust 規約 (`pbt 以下に unittest を書かない` / `PBT に「任意入力でパニックしないことだけを検証するテスト」を書かないこと`) に違反したテストが `pbt/tests/` 配下に多数存在する」と検出された。本 issue はその初回指摘 63 件 + 「PBT として残せる候補」とされたグレーゾーン 8 件 = **合計 71 件のみ** を対象に、磨き上げで再判定して整理する。

過去 issue `closed/0018-fix-remove-unittest-from-pbt.md` (2026-05-26 完了) は `pbt/tests/` 内の `#[cfg(test)] mod tests` ブロック 9 件を `tests/` に移管した。本 issue はその続編で、`proptest!` マクロ内に紛れ込んでいる「プロパティではないテスト」を対象とする。

本 issue は対象 71 件を以下 6 種に分類して処理する。

| 分類 | 件数 | 処置 |
|---|---:|---|
| A. Fuzz で代替可能 | 3 | PBT 側削除のみ |
| B. PBT としても fuzz としても無意味 | 6 | PBT 側削除のみ |
| C. 既存 `tests/test_*.rs` と意味的重複または部分重複 | 8 | PBT 側削除 + 一部は網羅補強で `tests/` 側に追加 |
| D. 単体テスト相当 (新規移植) | 33 | PBT 側削除 + `tests/test_*.rs` に新規追加 |
| PBT 残置 (再判定で規約準拠と判明) | 17 | 触らない |
| グレーゾーン (本 issue でも判定保留) | 4 | 触らない (本 issue 完了後に別 issue で再判定) |

検算: `A 3 + B 6 + C 8 + D 33 + PBT 残置 17 + グレーゾーン 4 = 71`。合計 PBT 側削除件数 = `A + B + C + D = 50`、`tests/` 側へ新規追加 = `D 33 + C 網羅補強 7 = 40`。

`pbt/tests/prop_event.rs:171` `prop_window_update_classification` は `/review-code` 対象外だが、本 issue の判定基準で関係式 PBT に該当するため、`## PBT 残置` セクションに「対象外として触らない」1 行を加える (対象 71 件にはカウントしない)。

スコープ明示: `pbt/tests/` 配下の `prop_*` 関数は全 231 件 (2026-06-10 時点 grep)。本 issue は上記 71 件のみが対象で、残り 160 件は `/review-code` 初回判定で扱われなかったため判定外。残り 160 件の再判定は **本 issue 完了後に別 issue で実施** する (詳細は `## スコープ外`)。

## 優先度根拠

Medium。CI および動作には影響しないが、

- shiguredo-rust 規約 (PBT の役割「ラウンドトリップ等のプロパティ検証」) に明確に違反している
- 規約違反テストが残っていると、それを真似た新規テストが追従して規約違反が拡大する
- 過去 issue 0018 の続編としての残作業

## 削除対象

### A. Fuzz で代替可能 (3 件)

`fuzz/fuzz_targets/` 内の対応ターゲットを実物で確認し、`&[u8]` 全体空間に対するパニック耐性を同等以上に検証していることを確認済み。PBT 側削除のみ。

| # | ファイル:行 (fn 行) | 関数 | 代替する fuzz ターゲット |
|---|---|---|---|
| A1 | `pbt/tests/prop_frame/main.rs:275` | `prop_decoder_robustness` | `fuzz/fuzz_targets/fuzz_frame_decoder.rs` (`decoder.feed(data)` + ループで全フレームをデコードし尽くす) |
| A2 | `pbt/tests/prop_hpack/main.rs:115` | `prop_hpack_decoder_robustness` | `fuzz/fuzz_targets/fuzz_hpack_decoder.rs` (上限なし + 上限ありの 2 系統で検証、`max_header_list_size` 違反のアサーション付き) |
| A3 | `pbt/tests/prop_webtransport/main.rs:227` | `prop_decode_incomplete_safe` | `fuzz/fuzz_targets/fuzz_capsule_decoder.rs` (`decoder.feed(data)` + ループで全 Capsule をデコードし尽くす) |

### B. PBT としても fuzz としても無意味 (6 件)

`prop_assert` を持たない、または `#[derive(Debug)] / #[derive(Clone)] / #[derive(Display)]` の挙動を確認するだけ。fuzz でも単体テストでも価値がない。PBT 側削除のみ。

| # | ファイル:行 (fn 行) | 関数 | 削除理由 |
|---|---|---|---|
| B1 | `pbt/tests/prop_event.rs:209` | `prop_event_debug_not_panic` | `#[derive(Debug)]` の panic 確認は不要 |
| B2 | `pbt/tests/prop_error.rs:118` | `prop_error_code_display_not_empty` | `format!("{}", code)` が空でないことを確認するだけ |
| B3 | `pbt/tests/prop_error.rs:136` | `prop_error_kind_display_not_empty` | 同上 |
| B4 | `pbt/tests/prop_settings.rs:75` | `prop_valid_settings_accepted` | 本体は `Settings::default()` と `apply(setting)` の 2 行で `prop_assert` を持たない |
| B5 | `pbt/tests/prop_frame/main.rs:142` | `prop_settings_frame_roundtrip` | Strategy は `ack in any::<bool>()` のみで `sf.is_ack() == ack` しか検証していない。SETTINGS ペイロード自体のラウンドトリップは `pbt/tests/prop_frame/main.rs:569` `prop_settings_frame_values_roundtrip` (`prop::collection::vec(valid_setting(), 1..10)` でラウンドトリップ) で既にカバー済みのため、本テストは冗長 |
| B6 | `pbt/tests/prop_event.rs:216` | `prop_event_clone_equality` | `#[derive(Clone, PartialEq)]` の動作確認。Strategy で振る入力に依存せず常に `e == e.clone()` が成立するため PBT として価値なし。新規 variant 追加時の網羅性が必要なら別 issue で `tests/test_event.rs` に variant 列挙テストを追加 |

B1 削除後も `any_event()` (`pbt/tests/prop_event.rs:138`) は B6 削除と合わせて参照ゼロになる。設計方針 4 のデッドコード除去で併せて削除する。

### C. 既存 `tests/test_*.rs` と意味的重複または部分重複 (8 件)

過去 issue 0018 などで `tests/test_*.rs` 側に同等の単体テストが既に存在する。**重複の度合いに応じて処置を分ける**。

| # | ファイル:行 (fn 行) | 関数 | 既存 tests/ 側 | 網羅性差分 | 処置 |
|---|---|---|---|---|---|
| C1 | `pbt/tests/prop_validation.rs:237` | `prop_duplicate_pseudo_header_rejected` | `tests/test_validation.rs:49` `test_duplicate_method` | PBT 側は `:method` / `:scheme` / `:path` 3 種、既存は `:method` のみ | PBT 削除 + `tests/test_validation.rs` に `test_duplicate_scheme` と `test_duplicate_path` を追加 |
| C2 | `pbt/tests/prop_validation.rs:265` | `prop_pseudo_after_regular_rejected` | `tests/test_validation.rs:61` `test_pseudo_header_after_regular` | PBT 側は `:authority` 固定 1 値、既存は `:scheme` 固定 1 値で検査対象が異なる | PBT 削除 + `tests/test_validation.rs` に `test_pseudo_authority_after_regular` を追加 |
| C3 | `pbt/tests/prop_validation.rs:309` | `prop_trailers_with_pseudo_rejected` | `tests/test_validation.rs:202` `test_trailers_with_pseudo_header` | PBT 側は 5 種 (`:status / :method / :path / :scheme / :authority`)、既存は `:status` のみ | PBT 削除 + `tests/test_validation.rs` に `test_trailers_with_pseudo_method` / `_path` / `_scheme` / `_authority` の 4 本を追加 |
| C4 | `pbt/tests/prop_validation.rs:556` | `prop_response_status_101_rejected` | `tests/test_validation.rs:208` `test_response_status_101_disallowed` | 既存テストで網羅済み | PBT 削除のみ |
| C5 | `pbt/tests/prop_connection/main.rs:68` | `prop_rst_stream_on_idle_is_error` | `tests/test_connection.rs:112` `test_rst_stream_on_idle_is_error` (0018 移管済み) | 既存テストで網羅済み | PBT 削除のみ |
| C6 | `pbt/tests/prop_connection/headers.rs:51` | `prop_continuation_without_headers_is_error` | `tests/test_connection.rs:83` `test_continuation_without_headers_is_error` (0018 移管済み) | 既存テストで網羅済み | PBT 削除のみ |
| C7 | `pbt/tests/prop_connection/settings.rs:18` | `prop_client_rejects_enable_push_from_server` | `tests/test_connection.rs:253` `test_client_rejects_enable_push_from_server` (0018 移管済み) | 既存テストで網羅済み | PBT 削除のみ |
| C8 | `pbt/tests/prop_error.rs:88` | `prop_known_error_code_from_u32` | `tests/test_error.rs:71` `test_known_error_codes_mapping` (0018 移管済み、17 値完全カバー) | 既存テストで網羅済み | PBT 削除のみ |

C 区分の網羅補強で `tests/` 側に追加するテスト数: C1 で 2 本 + C2 で 1 本 + C3 で 4 本 = **7 本**。

### D. 単体テスト相当 (新規移植、33 件)

PBT として価値がなく、かつ既存 `tests/test_*.rs` に同等テストが無いもの。`tests/test_<module>.rs` に新規追加する。テスト件数は `D1`〜`D28` のラベル管理だが、`D25` は 6 件まとめラベル。内訳件数は `D1`〜`D24, D26-D28` の 27 + `D25` の 6 = 33 件。

すべての移植テストは `HeaderField::new` / `shiguredo_http2` 公開 API のみで構築可能 (`pbt::wire_header_field` は不要)。

| # | ファイル:行 (fn 行) | 関数 | 移植先 | RFC 引用 |
|---|---|---|---|---|
| D1 | `pbt/tests/prop_validation.rs:358` | `prop_extended_connect_without_scheme_rejected` | `tests/test_validation.rs` (既存追記) | RFC 8441 §4 |
| D2 | `pbt/tests/prop_validation.rs:374` | `prop_extended_connect_without_path_rejected` | 同上 | RFC 8441 §4 |
| D3 | `pbt/tests/prop_validation.rs:429` | `prop_protocol_on_non_connect_rejected` | 同上 | RFC 8441 §4 |
| D4 | `pbt/tests/prop_validation.rs:637` | `prop_connect_authority_without_port_rejected` | 同上 | RFC 9113 §8.5 |
| D5 | `pbt/tests/prop_validation.rs:666` | `prop_connect_ipv6_authority_accepted` | 同上 | RFC 9113 §8.5 |
| D6 | `pbt/tests/prop_validation.rs:680` | `prop_asterisk_path_on_non_options_rejected` | 同上 | RFC 9113 §8.3.1 |
| D7 | `pbt/tests/prop_validation.rs:702` | `prop_asterisk_path_on_options_accepted` | 同上 | RFC 9113 §8.3.1 |
| D8 | `pbt/tests/prop_limits.rs:30` | `prop_default_build_succeeds` | `tests/test_limits.rs` (新規) | — |
| D9 | `pbt/tests/prop_settings.rs:271` | `prop_default_values` | `tests/test_settings.rs` (新規) | RFC 9113 §6.5.2 |
| D10 | `pbt/tests/prop_error.rs:143` | `prop_error_kind_with_code_display_contains_code` | `tests/test_error.rs` (既存追記) | — |
| D11 | `pbt/tests/prop_flow_control.rs:125` | `prop_add_recv_window_zero_rejected` | `tests/test_flow_control.rs` (既存追記) | RFC 9113 §6.9 |
| D12 | `pbt/tests/prop_connection/data.rs:18` | `prop_data_on_idle_stream_is_error` | `tests/test_connection.rs` (既存追記) | RFC 9113 §5.1 |
| D13 | `pbt/tests/prop_connection/headers.rs:117` | `prop_initial_headers_without_pseudo_is_error` | 同上 | RFC 9113 §8.1 / §8.3.1 |
| D14 | `pbt/tests/prop_connection/headers.rs:155` | `prop_invalid_hpack_causes_compression_error` | 同上 | RFC 9113 §4.3 |
| D15 | `pbt/tests/prop_connection/main.rs:96` | `prop_server_rejects_even_stream_id` | 同上 | RFC 9113 §5.1.1 |
| D16 | `pbt/tests/prop_connection/main.rs:126` | `prop_non_monotonic_stream_id_is_error` | 同上 | RFC 9113 §5.1.1 |
| D17 | `pbt/tests/prop_connection/main.rs:172` | `prop_window_update_on_idle_stream_is_error` | 同上 | RFC 9113 §5.1 |
| D18 | `pbt/tests/prop_connection/main.rs:203` | `prop_start_stream_after_goaway_is_error` (`_dummy`) | 同上 | RFC 9113 §6.8 |
| D19 | `pbt/tests/prop_connection/main.rs:282` | `prop_server_rejects_frame_without_preface` (`_dummy`) | 同上 | RFC 9113 §3.4 |
| D20 | `pbt/tests/prop_connection/main.rs:302` | `prop_server_accepts_frame_after_preface` (`_dummy`) | 同上 | RFC 9113 §3.4 |
| D21 | `pbt/tests/prop_connection/main.rs:320` | `prop_server_cannot_start_stream` (`_dummy`) | 同上 | RFC 9113 §8.4 |
| D22 | `pbt/tests/prop_connection/main.rs:477` | `prop_goaway_graceful_shutdown` (`_dummy`、固定 1 ケース) | 同上 | RFC 9113 §6.8 |
| D23 | `pbt/tests/prop_connection/settings.rs:40` | `prop_first_frame_must_be_settings` (`_dummy`) | 同上 | RFC 9113 §3.4 |
| D24 | `pbt/tests/prop_connection/settings.rs:62` | `prop_invalid_initial_window_size_is_flow_control_error` | 同上 | RFC 9113 §6.5.2 |
| D25 | `pbt/tests/prop_frame/main.rs:686, 709, 731, 750, 882, 1125` | stream_id=0 / increment=0 エラー系 6 件 (`prop_data_stream_id_zero_error`, `prop_headers_stream_id_zero_error`, `prop_rst_stream_stream_id_zero_error`, `prop_continuation_stream_id_zero_error`, `prop_window_update_zero_increment_error`, `prop_priority_stream_id_zero_error`) | `tests/test_frame.rs` (新規) | RFC 9113 §6.1 / §6.2 / §6.4 / §6.10 / §6.9 + §6.3 |
| D26 | `pbt/tests/prop_frame/main.rs:1229` | `prop_encoded_frame_type_correct` (9 フレーム種別テーブル駆動) | 同上 | RFC 9113 §4.1 |
| D27 | `pbt/tests/prop_frame/main.rs:1544` | `prop_decoder_clear_resets_state` | 同上 | — |
| D28 | `pbt/tests/prop_webtransport/main.rs:665` | `prop_duplicate_operations_are_errors` | `tests/test_webtransport/root.rs` (既存追記) | draft-ietf-webtrans-http2-14 §6.2 / §6.3 |

D 区分のうち実装で注意すべき点:

- **D24** の Strategy `(MAX_INITIAL_WINDOW_SIZE + 1)..=u32::MAX` は本実装の `Setting::from_wire` が範囲外を一律に弾く設計のため、tests/ 側では境界値 (`MAX_INITIAL_WINDOW_SIZE + 1` と `u32::MAX`) の 2 値ループで十分。
- **D25** は 6 件の `#[test]` をそれぞれ書く。元 PBT で payload が可変長 (DATA / HEADERS / CONTINUATION) のテストはループで「payload 長 0 / 短 / 長 / 典型値の 4 ケース」を回す。RST_STREAM (payload 4 バイト、`error_code` ランダム化) と PRIORITY (payload 5 バイト、`stream_dependency` / `weight` ランダム化) は本旨「stream_id=0 はエラー」と独立なので固定 1 ケースで十分。WINDOW_UPDATE は元 PBT が `stream_id in 0..=0x7FFF_FFFFu32` をランダム化していたため、stream_id を「0 / 任意の非ゼロストリーム ID」の 2 ケースで回す (本旨「increment=0 はエラー」を stream_id 両方で確認)。
- **D26** は 9 フレーム種別のテーブル駆動。元 PBT が使う `valid_stream_id()` Strategy は tests/ 側で `NonZeroStreamId::from_static(1)` 固定に置換、`WindowIncrement::new(1)` 等のヘルパー値も固定値で書く。
- **D27** の Strategy `partial_data in arbitrary_bytes(50)` は内部状態に影響しないため、固定 50 バイトの代表 1 ケースで十分。
- **D28** の移植先は `tests/test_webtransport/root.rs` (既存、`test_close_double_call_errors` などの層)。`tests/test_webtransport/main.rs` は `mod` 宣言のみのルーティングファイルなので追記しない。

## PBT 残置 (17 件 + 対象外明示 1 件 = リスト 18 件)

`/review-code` 初回判定で「単体テスト相当」または「PBT として残せる候補」とされたが、本 issue 磨き上げで shiguredo-rust 規約の PBT カテゴリ (ラウンドトリップ / 不変条件 / 関係式 / 同値性 / 状態遷移 / 対称性 / 冪等性 / 単調性 / 吸収状態) のいずれかに該当すると確認したテスト 17 件 + 元から対象外だが触らないことを明示するテスト 1 件。**本 issue では削除も移植もしない**。

| ファイル:行 (fn 行) | 関数 | カテゴリ | 対象 71 件への該当 |
|---|---|---|---|
| `pbt/tests/prop_event.rs:145` | `prop_stream_level_event_has_stream_id` | 関係式 | 該当 |
| `pbt/tests/prop_event.rs:151` | `prop_stream_level_event_not_connection_level` | 関係式 | 該当 |
| `pbt/tests/prop_event.rs:157` | `prop_connection_level_event_is_connection_level` | 関係式 | 該当 |
| `pbt/tests/prop_event.rs:163` | `prop_connection_level_event_has_no_stream_id` | 関係式 | 該当 |
| `pbt/tests/prop_event.rs:171` | `prop_window_update_classification` | 関係式 (stream_id == 0 で接続レベル) | **対象外** (元 PBT で /review-code 判定外、明示のため掲載) |
| `pbt/tests/prop_event.rs:188` | `prop_stream_id_value_matches` | 同値性 (7 variant が同じ stream_id を返す) | 該当 |
| `pbt/tests/prop_flow_control.rs:17` | `prop_flow_control_init` | 関係式 | 該当 |
| `pbt/tests/prop_flow_control.rs:164` | `prop_update_initial_preserves_recv_initial` | 不変条件 | 該当 |
| `pbt/tests/prop_stream/state.rs:205` | `prop_idle_to_open_via_headers` | 状態遷移 | 該当 |
| `pbt/tests/prop_stream/state.rs:227` | `prop_half_closed_symmetry` | 状態遷移対称性 | 該当 |
| `pbt/tests/prop_stream/state.rs:294` | `prop_open_data_transitions` | 状態遷移 | 該当 |
| `pbt/tests/prop_stream/state.rs:331` | `prop_half_closed_to_closed` | 状態遷移 | 該当 |
| `pbt/tests/prop_connection/settings.rs:90` | `prop_no_rfc7540_priorities_change_is_error` | 対称性 | 該当 |
| `pbt/tests/prop_connection/settings.rs:290` | `prop_enable_connect_protocol_intra_frame_duplicate` | 対称性 | 該当 |
| `pbt/tests/prop_settings.rs:194` | `prop_settings_idempotent` | 冪等性 | 該当 (グレーゾーンから再判定で PBT 残置へ移動) |
| `pbt/tests/prop_webtransport/main.rs:484` | `prop_session_state_always_valid` | 不変条件 | 該当 (グレーゾーンから移動) |
| `pbt/tests/prop_webtransport/main.rs:518` | `prop_session_closed_is_absorbing` | 状態遷移吸収状態 | 該当 (グレーゾーンから移動) |
| `pbt/tests/prop_webtransport/main.rs:557` | `prop_session_state_monotonicity` | 単調性 | 該当 (グレーゾーンから移動) |

対象 71 件への該当数 = 17 件 (PBT 残置の検算と一致)。リスト合計 18 件のうち 1 件 (`prop_window_update_classification`) は元 PBT で `/review-code` 対象外であり、対象 71 件には含めないが、本 issue の判定基準で関係式 PBT に該当するため触らないことを明示するために掲載する。

## グレーゾーン (4 件、本 issue でも判定保留)

以下 4 件は判定が割れ、本 issue で結論を出さない。`## スコープ外` で別 issue 化を確定する。

- `pbt/tests/prop_error.rs:191` `prop_error_display_contains_kind`
- `pbt/tests/prop_connection/main.rs:351` `prop_request_response_cycle`
- `pbt/tests/prop_connection/main.rs:420` `prop_ping_echo`
- `pbt/tests/prop_connection/main.rs:504` `prop_rst_stream_cancels_stream`

## 設計方針

### 1. 移植時の変換ルール

- `proptest! { #[test] fn prop_xxx(...) { ... prop_assert!(...) } }` → `#[test] fn test_xxx() { ... assert!(...) }` の機械的置換
- `prop_assert!` → `assert!`、`prop_assert_eq!` → `assert_eq!`
- `_dummy in Just(())` および固定値リテラルに置き換え可能な Strategy 引数は除去
- `prop_oneof![Just(A), Just(B), ...]` テーブル駆動: 列挙数によらず `for &case in &[A, B, ...]` のループで 1 本にまとめる (closed/0018 の `test_known_error_codes_mapping` に倣う)
- 装飾 Strategy (ランダム引数だが検証経路に効かない型) は代表値 1 つを固定。可変長コレクション付き Strategy (例: `prop::collection::vec(..., 0..=4)`) は「空 / 1 件 / 上限」の 3 ケースをループで回す
- **複数軸 `prop_oneof` の組み合わせは主軸 (テストの検証対象軸) のみループ展開、それ以外は固定値**。例: D3 (`prop_protocol_on_non_connect_rejected`) は method 軸のみ 4 ケースループ、scheme は `"https"` 固定、path は `"/"` 固定
- 装飾 Strategy の固定値対応表:

  | Strategy | 固定値 |
  |---|---|
  | `client_stream_id()` (奇数 ID 100 通り) | `NonZeroStreamId::from_static(1)` |
  | `(1u32..=100).prop_map(|n| n * 2)` (D15 偶数 ID) | `NonZeroStreamId::from_static(2)` |
  | `valid_stream_id()` (1..=0x7FFF_FFFF) | `NonZeroStreamId::from_static(1)` |
  | `http_scheme()` (`"http"` / `"https"`) | `"https"` |
  | `http_path()` (任意 path) | `"/"` |
  | `http_status()` (12 種) | テスト本旨に応じて `"200"` 等 |
  | `error_code_strategy()` (既知 17 + Unknown 1 = 18 種) | テスト本旨に応じて `ErrorCode::ProtocolError` 等 |
  | `valid_window_size()` (1..=2^31-1) | `65535` (DEFAULT_INITIAL_WINDOW_SIZE) |
  | `small_varint_value()` (0..=16383) | `42` |
  | `port in 1u16..=65535u16` (D5) | `443` |

- 関数名は `prop_xxx` → `test_xxx` に機械的置換。重複 (区分 C) は新規追加せず PBT 側削除のみ。網羅補強 (C1/C2/C3) で追加するテストは `test_<具体名>` で新規命名
- テストメッセージ (`assert!` の第 2 引数等) は日本語化する (`CLAUDE.md` 規約)

### 2. RFC 引用の保全

移植元 PBT の doc コメントに含まれる RFC 引用 (節番号、MUST / SHOULD / MAY) は必ず移植先 `tests/test_*.rs` の `#[test]` 関数の doc コメントへ転記する。転記時に `refs/` 配下の一次資料 (RFC 9113 / 9110 / 9112 / 8441 / 9218 / draft-ietf-webtrans-http2-14) を再確認する。

### 3. ヘルパー関数

- `pbt/tests/` 内の private/`pub(crate)` ヘルパー (`encode_frame`、`create_continuation`、`encode_valid_request_headers`、`setup_client_server`、`client_stream_id`、`valid_stream_id` 等) は触らない (PBT 残置テストで引き続き使用)
- `tests/test_connection.rs:15, 22` の `encode_frame` / `create_continuation` ローカル定義 (closed/0018 の成果) を D12〜D24 で再利用する
- D22 (`prop_goaway_graceful_shutdown`) は `shiguredo_http2::{frame::GoawayFrame, LastStreamId}` の追加 import が必要 (現在の `tests/test_connection.rs` の use 文に未含有)。D18 (`prop_start_stream_after_goaway_is_error`) は `client.start_stream(headers, true)` を呼ぶため `HeaderField` の use 追加が必要
- `pbt::wire_header_field` (`pbt/src/lib.rs:8`) は tests/ から呼べないが、本 issue の D 区分はすべて `HeaderField::new` で構築可能なため影響なし

D 区分のテスト関数ごとに必要な対応:

| 対象 | 元 PBT のヘルパー依存 | tests/ 側での対応 |
|---|---|---|
| D12-D14, D17 | `super::encode_frame`、`super::client_stream_id` | `tests/test_connection.rs:15` の `encode_frame` を再利用。`client_stream_id` (奇数ストリーム ID) は **値が検証経路に影響しない** ため `NonZeroStreamId::from_static(1)` に固定置換 |
| D13, D14 | `super::encode_frame`、`HpackEncoder` | `tests/test_connection.rs` 内で `HpackEncoder::new(4096)` を inline で使う (`encode_valid_request_headers` は使わない、各テストで必要な header set を直接 encode) |
| D15, D16 | `super::encode_frame`、`headers::encode_valid_request_headers` | `tests/test_connection.rs` に **`encode_valid_request_headers` 相当のローカルヘルパーを 1 つ追加複製する** (`HpackEncoder::new(4096)` + `:method GET / :scheme https / :path / / :authority example.com` の 4 ヘッダー、`Vec<u8>` を返す 10 行程度の private fn)。本ヘルパーは D15/D16 の 2 件で共用 |
| D18-D21, D22, D23 | `super::encode_frame`、(D22 のみ) `setup_client_server` | D22 は `setup_client_server` を **複製せず inline 展開**。`Connection::client(Limits::default()) → initiate() → サーバー側 SETTINGS を擬似 feed → process() で Active 状態に遷移 → GOAWAY を feed → process() でイベント発火確認` の順序で操作する (元 PBT の `setup_client_server` は完全ハンドシェイクを行うが、D22 の検証範囲では client 側の単体操作で十分)。D18-D21, D23 は元 PBT も単体 `Connection` 操作だけなので同パターン |
| D24 | `super::encode_frame`、`raw バイト` | 元 PBT は SETTINGS のフレームヘッダーを 9 バイト直接組み立てている。tests/ 側でも raw バイトを直接組む |
| D25 | `raw バイト` (FrameEncoder では stream_id=0 / increment=0 が型レベルで弾かれる) | `tests/test_frame.rs` に **`fn build_frame_bytes(frame_type: u8, flags: u8, stream_id: u32, payload: &[u8]) -> Vec<u8>` を 1 つ定義** (9 バイトフレームヘッダーを組み立てる 15 行程度の private fn)。6 件で共用 |
| D26, D27 | `super::encode_frame`、`super::valid_stream_id`、`super::arbitrary_bytes` | `tests/test_frame.rs` で `FrameEncoder` 公開 API を直接使う。`valid_stream_id` は `NonZeroStreamId::from_static(1)` に固定置換、`arbitrary_bytes` は固定 50 バイトの代表値 |
| D28 | `CapsuleEncoder`、`WtSession` (`small_varint_value` Strategy 含む) | `tests/test_webtransport/root.rs` の既存テスト (`test_close_double_call_errors` 等) と同様に `WtSession::client(WtConfig::default())` から構築。`small_varint_value` は `42` 等の代表値固定 |
| D1-D11 (validation, limits, settings, error, flow_control) | `HeaderField::new` / 各モジュールの公開 API | 独自ヘルパーは不要 |

### 4. 削除に伴うデッドコード除去

事前 grep で確認したデッドコード化候補。実装時に `cargo clippy --workspace --all-targets -- -D warnings` で再検出し、追加発見分も併せて削除する。

- `pbt/tests/prop_event.rs`: B1 (`prop_event_debug_not_panic`) と B6 (`prop_event_clone_equality`) を削除すると `any_event()` (L138) が参照ゼロになるため削除する。`stream_level_event()` / `connection_level_event()` / `error_code_strategy()` / `header_field_strategy()` は PBT 残置 (`prop_stream_level_event_*` 4 件、`prop_window_update_classification`、`prop_stream_id_value_matches`) で引き続き参照されるため残す
- `pbt/tests/prop_error.rs`: C8 (`prop_known_error_code_from_u32`) 削除に伴い `known_error_code_value` (L10) が参照ゼロになるため削除する。`error_code_strategy()` (L41) は他テスト (`prop_error_code_roundtrip` 等、本 issue 対象外) で引き続き参照されるため残す
- `pbt/tests/prop_frame/main.rs`: B5 (`prop_settings_frame_roundtrip`) 削除後も `valid_setting()` Strategy は他 PBT (本 issue 対象外) で引き続き参照されるため残す。`valid_stream_id()` / `arbitrary_bytes()` 等の汎用ヘルパーも引き続き多くの PBT で参照される
- `pbt/tests/prop_settings.rs`: B4 削除後も `valid_setting()` は L194 `prop_settings_idempotent` (PBT 残置) と L366 `prop_setting_wire_roundtrip` (本 issue 対象外) で引き続き使用されるためデッド化しない
- 他のファイル: 事前 grep ではデッドコード化候補は検出されなかった

### 5. 新規作成が必要なテストファイル

| ファイル | 用途 |
|---|---|
| `tests/test_limits.rs` | D8 |
| `tests/test_settings.rs` | D9 |
| `tests/test_frame.rs` | D25-D27 |

`pbt/tests/prop_stream/state.rs` 由来の 4 件はすべて PBT 残置となり、`tests/test_stream/state.rs` の新規作成は本 issue では不要。

### 6. 実装手順

本 issue は 1 PR で実施する (規模は closed/0036 より小さく、Phase 分割は不要)。ただし PR レビューの粒度を保つため commit 分割は任意で、以下の順序を推奨する。

1. ブランチ作成: `git switch -c feature/refactor-relocate-pbt-non-property-tests develop`
2. (推奨 commit 1) A 区分 3 件 + B 区分 6 件削除 + 設計方針 4 の事前 grep で確定したデッドコード (`any_event()`、`known_error_code_value`) 削除
3. (推奨 commit 2) C 区分 8 件削除 + 網羅補強 7 本 (C1/C2/C3) を `tests/test_validation.rs` に追加
4. (推奨 commit 3) D 区分 33 件削除 + 33 件再実装 + 新規ファイル 3 つ作成
5. 各 commit で `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` をローカル実行
6. PR description に完了条件の各メトリクスを記録 → `gh pr create`

## 完了条件

- `pbt/tests/` 配下から本 issue 削除対象 50 件 (A 3 + B 6 + C 8 + D 33) がすべて削除されている
- D 区分 33 件が対応する `tests/test_*.rs` で再現されている (テストメッセージ日本語化、RFC 引用は doc コメントに保全)
- C 区分の網羅補強 7 本 (C1 で 2 本、C2 で 1 本、C3 で 4 本) が `tests/test_validation.rs` に追加されている
- 新規ファイル 3 つ (`tests/test_limits.rs`、`tests/test_settings.rs`、`tests/test_frame.rs`) が作成されている
- 削除に伴うデッドコード (`pbt/tests/prop_event.rs` の `any_event()`、`pbt/tests/prop_error.rs` の `known_error_code_value`、+ clippy が検出した追加分) が併せて削除されている
- 本 issue 後に対象 71 件のうち PBT 側に残る関数 (PBT 残置 17 + グレーゾーン 4 = 21 件) は shiguredo-rust 規約準拠であることを PR セルフレビューで確認する
- `cargo build --workspace` / `cargo test --workspace` が通る
- 移管前後の `cargo test --workspace` の passed 件数を PR description に記録する。整合の目安は `(移管前 passed) − 50 (A 3 + B 6 + C 8 + D 33 の削除) + 33 (D 区分の `#[test]` 再実装) + 7 (C 区分網羅補強) = (移管後 passed)`、すなわち `(移管後 passed) = (移管前 passed) − 10`。D25 はループ駆動で `#[test]` 6 本 + DATA/HEADERS/CONTINUATION は payload 長 4 ケース + RST_STREAM/PRIORITY は固定 1 ケース + WINDOW_UPDATE は stream_id 2 ケース、D26 は 1 本 + 9 論理 case、D24 は 1 本 + 2 論理 case と展開されるため、ループ展開後の論理 case 数も併記する (「`#[test]` 件数」と「論理 case 数」を分けて記録)
- `cargo clippy --workspace --all-targets -- -D warnings` が通る (デッドコード警告 0)
- `cargo fmt --all -- --check` が通る
- A1〜A3 削除前に PR 作成者が手元で `cargo fuzz run fuzz_frame_decoder -- -runs=100000` / `fuzz_hpack_decoder -- -runs=100000` / `fuzz_capsule_decoder -- -runs=100000` を 1 回実行し、各コマンドの最終行 (`#1000000 cov: X ft: Y corp: Z exec/s: W rss: V Mb` 形式) を PR description に貼り付ける (`cargo install cargo-fuzz` + nightly toolchain が必要。crash 発見時は本 issue を保留して別 issue 化)
- (任意) `cargo llvm-cov` のカバレッジ確認は CI に組み込まれていないため本 issue の完了条件には含めない。発見されたカバレッジ低下が懸念されれば別 issue (`refactor`: `cargo llvm-cov` を CI に追加する) で対応する
- `CHANGES.md` `## develop` の `### misc` に下記文面を追記する

```
- [UPDATE] pbt/tests/ から非プロパティテスト 50 件を削除し、うち 33 件を tests/test_*.rs に単体テストとして再実装、加えて C 区分の網羅補強 7 本を tests/test_validation.rs に追加する (issue 0067)
  - @voluntas
```

## スコープ外

- グレーゾーン 4 件 (`prop_error_display_contains_kind`, `prop_request_response_cycle`, `prop_ping_echo`, `prop_rst_stream_cancels_stream`) の処理 → 本 issue 完了後に別 issue を起こすか個別判断
- **`/review-code` 初回指摘範囲外の 160 件の再判定** → 本 issue 完了後に別 issue (`refactor`) を起こす。タイトル候補: `pbt/tests/ 配下の規約適合性を網羅的に再判定する`。動機: 本 issue は `/review-code` 初回指摘範囲 71 件のみを対象としたため、PBT 全 231 件のうち残り 160 件 (regex `^fn prop_` で確認) が判定外で残っている。次のフェーズで網羅再判定を行う
- B1 (`prop_event_debug_not_panic`) / B6 (`prop_event_clone_equality`) 削除に伴って「すべての Event variant の Debug 出力 / Clone 動作を網羅する単体テスト」の新規追加 (必要なら別 issue で `tests/test_event.rs` に追加)
- `pbt::wire_header_field` (`pbt/src/lib.rs:8`) を tests/ から使えるようにする整備 (本 issue では不要だが、`/review-code` 範囲外 160 件の整理で wire 模擬を tests/ 側でも使いたくなる場合は別 issue で検討)
- `pbt/tests/prop_connection/settings.rs:87-88` の RFC 9218 §2.1 の MAY/MUST 食い違い表現の修正 (PBT 残置の `prop_no_rfc7540_priorities_change_is_error` の doc コメント変更。本 issue は触らない)
- `pbt/tests/` 配下のヘルパー関数の重複整理 (tests/ 側と pbt/ 側で `encode_frame` 等が重複しているが本 issue では触らない)
- `pbt/tests/prop_frame/main.rs` のサブモジュール分割 (closed/0048 で「対応不要」と判断済み。本 issue 完了後の行数を測定して必要なら別 issue で判断)
- `crates/tokio-http2/`、`crates/tokio-nghttp2/`、`crates/shiguredo_nghttp2/`、`crates/nghttp2-sys/` 配下のテスト整理 (ルート crate のみが本 issue のスコープ)

## 解決方法

### 削除

- `pbt/tests/` 配下から非プロパティテスト 50 件を削除した (A 3 + B 6 + C 8 + D 33)
  - A 区分 (Fuzz で代替可能): `prop_decoder_robustness` (`pbt/tests/prop_frame/main.rs`)、`prop_hpack_decoder_robustness` (`pbt/tests/prop_hpack/main.rs`)、`prop_decode_incomplete_safe` (`pbt/tests/prop_webtransport/main.rs`)
  - B 区分 (PBT/fuzz としても無意味): `prop_event_debug_not_panic` / `prop_event_clone_equality` (`pbt/tests/prop_event.rs`)、`prop_error_code_display_not_empty` / `prop_error_kind_display_not_empty` (`pbt/tests/prop_error.rs`)、`prop_valid_settings_accepted` (`pbt/tests/prop_settings.rs`)、`prop_settings_frame_roundtrip` (`pbt/tests/prop_frame/main.rs`)
  - C 区分 (既存 `tests/` と重複): 8 件 (`pbt/tests/prop_validation.rs` / `prop_connection/main.rs` / `prop_connection/headers.rs` / `prop_connection/settings.rs` / `prop_error.rs` 各所)
  - D 区分 (単体テスト相当): 33 件
- デッドコード除去: `pbt/tests/prop_event.rs` の `any_event()`、`pbt/tests/prop_error.rs` の `known_error_code_value` を削除
- `pbt/tests/prop_connection/data.rs` は D12 削除後に空になったため `git rm` してファイルごと削除。`pbt/tests/prop_connection/main.rs` の `mod data;` 宣言も削除
- `pbt/tests/prop_connection/headers.rs` の `encode_valid_request_headers` ヘルパーは D15/D16 移植後に参照ゼロになるため削除

### 追加

- D 区分 33 件を `tests/test_*.rs` に単体テストとして再実装した
  - D1〜D7 → `tests/test_validation.rs` (Extended CONNECT / CONNECT authority-form / asterisk path 等)
  - D8 → `tests/test_limits.rs` (新規作成)
  - D9 → `tests/test_settings.rs` (新規作成)
  - D10 → `tests/test_error.rs`
  - D11 → `tests/test_flow_control.rs`
  - D12〜D24 → `tests/test_connection.rs` (idle ストリーム / 偶数 stream_id / 単調増加 / GOAWAY / preface 検証 / INITIAL_WINDOW_SIZE 範囲外)
  - D25〜D27 → `tests/test_frame.rs` (新規作成、stream_id=0 系 6 件 + frame_type 整合性 + decoder clear)
  - D28 → `tests/test_webtransport/root.rs`
- C 区分の網羅補強 7 本を `tests/test_validation.rs` に追加した (C1: `test_duplicate_scheme` / `test_duplicate_path`、C2: `test_pseudo_authority_after_regular`、C3: `test_trailers_with_pseudo_method` / `_path` / `_scheme` / `_authority`)
- `tests/test_connection.rs` に D15 / D16 用のローカルヘルパー `encode_valid_request_headers` を追加した
- `tests/test_frame.rs` に D25 用のローカルヘルパー `build_frame_bytes` / `assert_decode_protocol_error` を追加した

### 検証

- `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` がすべて通過することを確認した
- `/review-diff-code` ループを 1 周回し、致命的・重要レベルの指摘 (RFC 引用の誤り、`Vec::with_capacity`、テストメッセージの英語混入、ヘルパーのエラー種別未検証、移植プロセス由来のコメント残存) をすべて修正した
- `cargo fuzz run fuzz_frame_decoder/fuzz_hpack_decoder/fuzz_capsule_decoder` は本 issue の自動解決ではユーザー判断によりスキップした (issue 完了条件には fuzz 100,000 run 実行の指示があるが、`/auto-resolve` 実行時に PR description 添付をスキップする選択をユーザーから受けたため)。fuzz 自体は CI で別途検証される
