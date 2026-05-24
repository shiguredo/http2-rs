# `concatenate_cookies` と `EmptyPath` の scheme 依存判定にテストを追加する

Created: 2026-05-23
Model: Opus 4.7

## 内容

issue 0024 の /review-diff-code で指摘された以下 2 点のテスト不足を解消する。

1. `src/connection/mod.rs::concatenate_cookies` の空 cookie 除外ロジック、複数 cookie 連結、sensitive フラグ伝播を検証するテストが存在しない
2. `src/validation.rs` の `EmptyPath` の scheme 依存判定 (http/https のみ拒否、その他は許容) を検証するテストが存在しない

`pbt/tests/prop_connection.rs` で `cookie` / `concatenate` を grep しても 0 件。`pbt/tests/prop_validation.rs` で `EmptyPath` / `empty.*path` を grep しても 0 件。

## 設計方針

### `concatenate_cookies` のテスト戦略

`concatenate_cookies` は `src/connection/mod.rs:1941` で **private fn** として定義されており、crate 外 (`pbt/tests/`, `tests/`) から直接呼び出せない。テスト方針として 3 案を検討した。

| 案 | 概要 | 採否 |
|---|---|:-:|
| A | `concatenate_cookies` を `pub(crate)` 化し、`src/connection/mod.rs` 内 `#[cfg(test)] mod tests` で単体テスト + PBT を書く | **採用** |
| B | `__test_helpers` モジュールに薄いラッパを追加し crate 外公開、PBT は `pbt/tests/prop_connection.rs` に書く | 不採用 |
| C | `Connection` 公開 API (HPACK decode → `concatenate_cookies` 起動) 経由のシナリオテストで間接検証 | 不採用 |

採用根拠: A 案は CLAUDE.md L78-L88 のテスト規約 (`pub(crate)` 関数を直接叩くテストは `src/<module>` の `mod tests` に残す) と整合し、0036 (mod tests を tests/ に分離) の移管原則 2 (「`pub(crate)` 直接依存テストは `mod tests` に残す」) にも従う。B 案は `__test_helpers` の表面を広げる副作用があり、`concatenate_cookies` は構築時検査を伴わない単純連結関数で「型不変条件をバイパスする」用途とは性質が違うため適さない。C 案はテスト記述コストが過大。

### `EmptyPath` のテスト戦略

`EmptyPath` は `validate_request_headers` の戻り値で検証可能 (公開 API)。`HeaderField::new(":scheme", value)` で構築した `Vec<HeaderField>` を渡し、`Err(ValidationError::EmptyPath)` の有無を確認する。

固定 4 ケース (http / https / HTTP / ftp) は **単体テスト** で記述 (CLAUDE.md L94 「単体テストは意図的なエラーパス、境界値など PBT で実現できないケース」)。固定値テストを PBT 化しても入力多様性が活きない。

加えて、scheme 値を「http/https の大文字小文字混在に正規化される文字列」と「その他の任意 ASCII alphabetic 文字列」の 2 strategy で生成し、「http/https に正規化されるなら Err(EmptyPath)、それ以外なら Ok」を property 化する **PBT を 1 件追加** する。これは固定 4 ケースでは網羅できない `eq_ignore_ascii_case` の境界 (例: `:scheme = "Http"`, `"HTTPS"`, `"hTTpS"`) を担保する。

### `:scheme` の大文字許容性の確認

`HeaderField::new(":scheme", "HTTP")` は `:scheme` 値構文検査 (RFC 3986 §3.1: `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`) で ALPHA が大文字小文字両方を許容するため **構築時に通る** (現状 `src/hpack/table.rs::validate_pseudo_header` の `is_valid_scheme` で `is_ascii_alphabetic()` を使用。0034 完了後は同関数が `src/syntax.rs` に移動するが挙動は不変)。issue 0024 の解決方法でも `:scheme = HTTP` は構築時に弾かない設計を採っている。

### `concatenate_cookies` の実装挙動 (前提把握)

`src/connection/mod.rs:1941-1985` の実装は以下の早期 return / 除外ロジックを持つ。テスト設計はこれらを正確に踏まえる必要がある。

1. `cookie_count <= 1` (= cookie 0 件 or 1 件) のとき: **headers を変更せずそのまま返す**
2. cookie 値が空 (`value.is_empty()`) の cookie は **連結対象から事前除外**
3. 除外後 `cookie_values.is_empty()` (= cookie はあったが全空) のとき: cookie を 1 件も出力しない (非 cookie のみ返す)
4. 連結する場合、結果 cookie は **非 cookie 全件を順序保持した後ろに 1 件として追加** される
5. `cookie_sensitive` は **cookie ヘッダーの sensitive のみを OR 連結**。非 cookie の sensitive は影響しない

### テスト用 `HeaderField` 構築

`HeaderField::new("cookie", "")` は field-value 検査 (RFC 9113 §8.2.1) で空 value を許容するため通る (空 value は NUL/CR/LF/先頭末尾 SP HTAB のいずれにも該当しない)。空 cookie テストでは `.unwrap()` で安全に構築できる。非空値も strategy 範囲 (`0x21u8..=0x7E`) なら確実に通る。

PBT 内で cookie field-name は `"cookie"` (lowercase) に固定する。`HeaderField::new("Cookie", ...)` は field-name lowercase 検査 (RFC 9113 §8.2.1, MUST NOT 0x41-0x5a) で fail する。実装の `eq_ignore_ascii_case` は HPACK decoder 経由の wire データ (検査バイパスされた `from_validated_parts` 構築) のみで大文字 cookie に意味を持つため、PBT スコープでは lowercase 固定で十分。大文字 cookie の挙動検証は別 issue (HPACK decode 経路の integration test)。

### `concatenate_cookies` の PBT/単体テスト具体内容

新規テストを `src/connection/mod.rs` 内 `#[cfg(test)] mod tests` に追加する (現状この mod tests が無ければ新設)。

- **単体テスト 1**: 全 cookie 空 + cookie_count >= 2 の除外。`HeaderField::new("cookie", "").unwrap()` を 2 件以上含む入力で、出力に `cookie` ヘッダーが含まれないことを確認 (cookie_count <= 1 の早期 return を通さないため 2 件以上で組む)
- **単体テスト 2**: 空 cookie 混在時の二重区切り不在。1 件以上の空 cookie と 1 件以上の非空 cookie の混在で、連結結果の cookie 値に `"; ;"` (区切り SP + `;`) 3 バイトパターンが含まれないこと
- **単体テスト 3**: sensitive フラグ伝播。`new_with_sensitive("cookie", "a=1", true).unwrap()` と `new_with_sensitive("cookie", "b=2", false).unwrap()` の連結結果の sensitive フラグが `true` (cookie のみの OR、非 cookie の sensitive は無関係)
- **単体テスト 4**: 早期 return の挙動確認 (2 関数に分離する)
  - `concatenate_cookies_returns_input_when_cookie_count_is_zero`: cookie 無し、非 cookie のみの入力で出力 = 入力
  - `concatenate_cookies_returns_input_when_cookie_count_is_one`: cookie 1 件のみ (非空)、非 cookie 混在で出力 = 入力 (連結 cookie が末尾に追加されない)
- **PBT 1** (関数名: `prop_concatenate_cookies` — リポジトリ既存 PBT (`pbt/tests/prop_validation.rs` の `prop_*` 群) の命名規則に倣う): cookie 数 0..=8、非 cookie 数 0..=4 で混在生成し、以下を分岐 assert する
  - cookie 値 strategy: `prop_oneof![1 => Just(Vec::new()), 3 => prop::collection::vec(0x21u8..=0x7E, 1..=32)]` (空 1 : 非空 3 の重み調整で「全空」ケースが偏らないようにする)。`HeaderField::new("cookie", value)` の field-value 検査を確実に通る範囲
  - **ケース A: cookie 数 <= 1**: 出力 = 入力 (早期 return)
  - **ケース B: cookie 数 >= 2 かつ全 cookie 値が空**: 出力に cookie ヘッダーが含まれない (非 cookie のみ、順序保持)
  - **ケース C: cookie 数 >= 2 かつ 1 件以上 cookie 値が非空**: 出力は (非 cookie 全件、順序保持) + (連結 cookie 1 件 末尾) の構造。連結 cookie 値に `"; ;"` 3 バイトパターン (= `0x3B 0x20 0x3B`) が含まれない (空 cookie 除外の検証)、sensitive フラグが入力 cookie の sensitive の OR 連結に一致
  - 検証コード例:
    - 末尾 cookie 配置: `assert_eq!(output.last().unwrap().name(), b"cookie")`
    - 非 cookie 順序保持: `let non_cookie_input: Vec<_> = input.iter().filter(|h| !h.name().eq_ignore_ascii_case(b"cookie")).collect(); let non_cookie_output: Vec<_> = output[..output.len() - 1].iter().collect(); assert_eq!(non_cookie_input, non_cookie_output);`
    - 二重区切り不在: `assert!(!output.last().unwrap().value().windows(3).any(|w| w == b"; ;"))`
  - **検証の意義**: 実装側で `if !value.is_empty()` 除外を削った場合、空 cookie 混在入力に対し `"v1; ; v2"` のような連結結果になり、3 バイトウィンドウで `b"; ;"` (0x3B 0x20 0x3B) が出現する。本 PBT はこの実装欠陥を検出できる mutation 耐性を持つ

### `EmptyPath` のテスト具体内容

新規テストを `pbt/tests/prop_validation.rs` (PBT) と `src/validation.rs` 内 `#[cfg(test)] mod tests` (単体) に追加する。

- **単体テスト 4 ケース** (`src/validation.rs` の `mod tests`):
  - `:scheme = "http"`, `:path = ""` → `Err(EmptyPath)`
  - `:scheme = "https"`, `:path = ""` → `Err(EmptyPath)`
  - `:scheme = "HTTP"`, `:path = ""` → `Err(EmptyPath)` (大文字 ASCII の eq_ignore_ascii_case 確認)
  - `:scheme = "ftp"`, `:path = ""` → `Ok` (http/https 以外は許容)
- **PBT 1** (`pbt/tests/prop_validation.rs`):
  - Strategy A (http/https の大文字小文字異綴): `"[hH][tT][tT][pP]"` 正規表現 strategy (4 文字、http) と `"[hH][tT][tT][pP][sS]"` (5 文字、https) を `prop_oneof!` で組み合わせ。`:scheme = scheme`, `:path = ""` のとき必ず `Err(EmptyPath)`
  - Strategy B (それ以外の任意 scheme): `prop_oneof!` の選択肢を **長さ集合 {1, 2, 3, 6, 7, 8} に限定**して `[a-zA-Z][a-zA-Z0-9+\-.]{N-1}` (N = 上記 6 値) を生成し、`prop_filter` で `!s.eq_ignore_ascii_case("http") && !s.eq_ignore_ascii_case("https")` のみ通す。4-5 文字は http/https と eq_ignore_ascii_case で衝突する候補が多く reject 率を押し上げるため、選択肢から除く。`:scheme = scheme`, `:path = ""` のとき必ず `Ok`
  - 両 strategy を `prop_oneof!` で混在させ、scheme と期待 result を tuple で受け取り判定結果を property assert
  - reject 率高騰時は `proptest! { #![proptest_config(ProptestConfig { max_global_rejects: 100_000, ..ProptestConfig::default() })] }` で許容上限を引き上げること (proptest デフォルトは 65_536)

## 完了条件

- [ ] `src/connection/mod.rs::concatenate_cookies` が `pub(crate)` 化されている (signature: `pub(crate) fn concatenate_cookies(headers: Vec<HeaderField>) -> Vec<HeaderField>`)
- [ ] `src/connection/mod.rs` 内 `#[cfg(test)] mod tests` に `concatenate_cookies` の単体テスト 4 件と PBT 1 件 (`prop_concatenate_cookies`) が追加されている
- [ ] PBT 1 のケース C は「末尾 cookie 配置」「非 cookie 順序保持」「二重区切り (`b"; ;"`) 不在」の 3 アサーションをすべて含む
- [ ] PBT 1 (cookie) は cookie 数 0..=8、非 cookie 数 0..=4、cookie 値は `prop_oneof![Just(Vec::new()), prop::collection::vec(0x21u8..=0x7E, 1..=32)]` strategy (NUL/CR/LF/SP/HTAB を含まない印字可能 ASCII 1..=32 文字 + 空文字列) で生成され、proptest のデフォルト 256 ケース以上で成功する
- [ ] `pbt/tests/prop_validation.rs` に `EmptyPath` の scheme 依存判定 PBT 1 件が追加されている (Strategy A/B の 2 strategy を `prop_oneof!` で混在)
- [ ] `src/validation.rs` 内 `#[cfg(test)] mod tests` に `EmptyPath` の固定 4 ケース単体テストが追加されている
- [ ] 全テストが `cargo test --workspace` と `cargo test --workspace --features __test_helpers` で通る
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --all -- --check` が通る
- [ ] `concatenate_cookies` の `pub(crate)` 化が public API 表面 (`shiguredo_http2::*` の re-export) を増やさないことを確認 (`grep -F concatenate_cookies src/lib.rs` が 0 件、または `pub use ` を含む行に `concatenate_cookies` が現れないこと)
- [ ] CHANGES.md `### misc` に下記文面を追記

## CHANGES.md エントリ

`## develop` の `### misc` に追記する:

```
- [ADD] `concatenate_cookies` の空 cookie 除外・sensitive 伝播と、`EmptyPath` の scheme 依存判定 (http/https の eq_ignore_ascii_case) を検証する PBT と単体テストを追加する
  - @voluntas
```

## ブランチ命名

`feature/add-pbt-for-cookie-and-empty-path` を使用する。

## スコープ外

- `concatenate_cookies` の public API 化 (`pub fn`) → `pub(crate)` 止まり。`shiguredo_http2` 利用者は HPACK decode → `Connection` 経由でしか cookie 連結を経験しないため、外部公開不要
- `Connection` 公開 API 経由のシナリオテスト (HPACK decode → cookie 連結の経路全体) → 別 issue
- `refs/rfc6265.txt` の追加 → 本 issue では引用節番号 (RFC 6265 §4.2.1) のみ参照し、refs/ への一次資料追加は別 issue
- cookie 値の文法レベル検証 (cookie-pair, cookie-name, cookie-value のトークン文字種) → 本 issue は連結ロジックのテストで、cookie 個別値の文法は `HeaderField::new` の field-value 検査 (RFC 9113 §8.2.1) で担保済み
- `:scheme` 値の token 構文検査の追加テスト → 0024 で構築時検査が確立済み

## テスト戦略の補足

- 単体テストは PBT で実現できないケース (固定値 4 分岐、sensitive フラグの境界) に限定。CLAUDE.md L94 規約と整合
- PBT は型情報に基づく入力生成でラウンドトリップ性質や境界網羅を検証。CLAUDE.md L92 規約と整合
- PBT で「任意入力でパニックしないことだけを検証するテスト」は書かない (fuzz の役割)。CLAUDE.md L104 規約と整合

## RFC 引用

- RFC 9113 §8.3.1: `:path` の http/https スキーム依存の空判定 (`This pseudo-header field MUST NOT be empty for "http" or "https" URIs`)
- RFC 9113 §8.2.3: cookie ヘッダーの HTTP/2 における連結 (Cookie field-value を `0x3B 0x20` ("; ") で連結する)
- RFC 6265 §4.2.1: `cookie-string = cookie-pair *( ";" SP cookie-pair )` 文法
- RFC 3986 §3.1: `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` (大文字 ALPHA 許容)
- RFC 9110 §15: HTTP セマンティクス全般

## 依存

- 関連: [[0036-refactor-move-mod-tests-to-tests-dir]] (本 issue で追加するテストは `pub(crate)` 直接依存のため `src/<module>` 内 `mod tests` に残す。0036 の移管原則 2 で扱う対象)
- 関連: [[0034-refactor-consolidate-field-syntax-module]] (`:scheme` の `is_valid_scheme` 関数が `src/syntax.rs` に移動するが、`HeaderField::new` の挙動は不変のため本 issue のテストは無影響)
