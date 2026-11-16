# shiguredo_http2 の WebTransport integration テストを分割する

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-split-webtransport-integration-tests
- Polished: 2026-11-16

## 目的

`tests/test_webtransport/integration.rs` が 2026-11-16 時点で 6291 行・136 テストを抱え、WebTransport の複数の関心が 1 ファイルに混在して可読性と保守性が低下している。既にディレクトリモジュール化されている `tests/test_webtransport/` のサブモジュールへ関心ごとに分割する。テスト本文とアサーションの意味は変えない。

## 現状

`tests/test_webtransport/integration.rs` は 2026-11-16 時点 (0117 適用前) で 6291 行、`#[test]` は 136 件である (issue 作成時の 4244 行・99 件から増加している)。次の関心が混在している。

- セッション / ストリームの基本操作 (open、送受信、状態 getter、クローズ後のエラー)
- フロー制御 (WT_MAX_DATA / WT_MAX_STREAM_DATA / WT_MAX_STREAMS と各 BLOCKED capsule)
- WT_RESET_STREAM と WT_STOP_SENDING の送受信
- 方向検証 (送信専用 / 受信専用ストリームへの操作)
- 削除済み・未作成 ID とピア開始 bidi の暗黙作成
- 受信状態の検証 (`Recv` / `DataRecvd` / `DataRead` / `ResetRead`)
- 引数の範囲検証 (error code と varint 上限)

`tests/test_webtransport/main.rs` は `capsule` / `error` / `exporter` / `flow_control` / `init` / `protocols` / `root` / `stream` / `varint` の 9 サブモジュールと `integration` を宣言しており、`integration.rs` だけが関心を束ねたままの状態になっている。`tests/test_webtransport/` の既存サブモジュールは 173 行 (`varint.rs`) から 682 行 (`init.rs`) に収まっており、`init.rs` と `capsule.rs` (628 行) は 500 行を超えている。本 issue は `integration.rs` のみを対象とし、既存サブモジュールは変更しない。

分割を扱う issue には `issues/0111-refactor-split-reset-stream-tests.md` (`tests/test_connection.rs` が対象) と `issues/0154-refactor-split-driver-webtransport-tests.md` (`crates/tokio-http2/tests/test_webtransport.rs` が対象) があるが、本ファイルはどちらの対象でもない。一方、`issues/0117-change-privatize-wt-config-fields.md` は `tests/test_webtransport/integration.rs` の `WtConfig` 構築箇所を変更するため、本 issue と対象ファイルが重なる。

## 設計方針

- `tests/test_webtransport/integration.rs` を削除し、関心ごとのサブモジュールに分割して `tests/test_webtransport/main.rs` の `mod` に追加する。`mod integration;` は削除する。ファイル名は内容が分かる英語スネークケースにする (既存サブモジュールと同じ命名規則)
- テスト本文・アサーション・テスト名は変更しない。ファイル移動に伴う差分を除けば、変更は `use` の並び、`mod` 宣言、共有ヘルパーと共有定数の移動を目標にする
- 分割単位は現状に挙げた関心を基本とする。1 サブモジュールが 500 行を超える関心は、テストの意味を変えずに同一関心内でさらに分割する。例えばフロー制御は WT_MAX_DATA / WT_MAX_STREAM_DATA / WT_MAX_STREAMS / BLOCKED capsule などに分ける。既存の `capsule.rs` (628 行) と `init.rs` (682 行) は本 issue の対象外とし、変更しない
- 帰属は各テストが直接検証している対象 (テスト名とアサーションの主対象) で決める。複数の関心にまたがるテストは、主検証対象の関心へ置く
- `main.rs` には `mod` 宣言、`use`、共有ヘルパー、共有定数だけを置き、`#[test]` を置かない。共有ヘルパー (`decode_single_capsule` と状態を作るヘルパー群) と共有定数 (`MAX_APP_ERROR_CODE`) は、複数のサブモジュールから使うものだけを `main.rs` に移す。1 つのサブモジュールでしか使わないものはそのファイルに残す
- 分割直前に `cargo test --test test_webtransport -- --list` でテスト名を実測し、分割後もテスト件数とテスト関数名の一覧 (モジュール経路を除く、重複を含む) が一致することを確認する。2026-11-16 時点では `integration.rs` が 136 件、ターゲット全体が 295 件である。0117 の完了後は件数が変わり得るため、絶対値は基準とせず分割直前の実測値を使う
- 0111 / 0154 とは対象ファイルが重ならないため、これらの完了を待たずに着手できる。0117 とは `tests/test_webtransport/integration.rs` で重なるため、0117 を先に完了させる。0117 より先に本 issue を実装する場合は、0117 側で分割後の各サブモジュールへ変更を適用する

## 完了条件

- `tests/test_webtransport/integration.rs` が削除され、`tests/test_webtransport/main.rs` から `mod integration;` が削除されていること
- `tests/test_webtransport/main.rs` に分割後の各サブモジュールの `mod` 宣言が追加されていること
- `integration.rs` から分割した各サブモジュールが 500 行以下で、1 つの関心 (500 行を超える関心をさらに分割した場合はその下位の関心) に対応していること
- `tests/test_webtransport/main.rs` に `#[test]` が無いこと
- 本 issue で既存の `capsule` / `error` / `exporter` / `flow_control` / `init` / `protocols` / `root` / `stream` / `varint` サブモジュールに変更を加えないこと
- テスト名とアサーションの意味が分割前と変わっていないこと (`git diff` でテスト本文の変更が無く、ファイル移動と `use` / `mod` / 共有ヘルパー / 共有定数の移動だけであることを確認する)
- `cargo test --all` が通過し、`test_webtransport` のテスト件数とテスト関数名の一覧 (モジュール経路を除く、重複を含む) が分割直前の実測値と一致すること
