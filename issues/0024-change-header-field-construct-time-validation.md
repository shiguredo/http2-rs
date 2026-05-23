# HeaderField を構築時検査型に変更する

Created: 2026-05-23
Model: Opus 4.7

## 概要

`HeaderField` を「不正な値を持てない型」に作り直す。現状の `HeaderField::new(name, value)` /
`HeaderField::from_str(name, value)` / `HeaderField::new_sensitive(name, value, sensitive)` /
`HeaderField::sensitive(name, value)` は無検査でフィールドを構築でき、さらに全フィールドが
`pub` であるため構造体リテラルや直接代入でも不正値を注入できる。

破壊的変更を伴う。`shiguredo_http2` の公開 API のうち `HeaderField` 構築系をすべて
`Result<Self, HeaderFieldError>` 化し、フィールドを private 化してアクセサを提供する。
加えてリテラル定数向けの `const fn` 構築 API (`HeaderField::from_static`) を提供して
**RFC 違反をコンパイル時に検出可能** にする。

## 設計方針: 二段の構築 API

利用者の入力源によって 2 つの構築点を提供する。

| API | 対象 | 検査タイミング | 失敗時の挙動 |
|---|---|---|---|
| `HeaderField::new(name, value)` | ランタイム値 (`impl AsRef<[u8]>`) | 実行時 | `Err(HeaderFieldError)` |
| `HeaderField::from_static(name, value)` | `&'static [u8]` リテラル | コンパイル時 (`const fn`) | コンパイルエラー (const eval panic) |

`from_static` を `const fn` で実装することで、以下のような定数定義はリテラルが
RFC 違反なら **CI を回す前にコンパイルエラー** で検出される。

```rust
// OK: コンパイル成功
const METHOD: HeaderField = HeaderField::from_static(b":method", b"GET");

// NG: コンパイル時に "field-name must be lowercase" で fail
const BAD: HeaderField = HeaderField::from_static(b"Host", b"example.com");

// NG: コンパイル時に "field-value contains CR" で fail
const INJECT: HeaderField = HeaderField::from_static(b":path", b"/foo\r\nX-Inject: 1");
```

`from_static` は `sensitive: false` 固定とする。`const fn` で `bool` 引数自体は受け取れるが、
静的定数として定義するヘッダーに `sensitive: true` を付けるユースケースは稀であるため、
API を簡潔に保つ。`sensitive: true` が必要な場合は `new_with_sensitive` 経由で構築する。

## 背景

現状のコードでは以下の問題がある:

- `HeaderField::from_str(":path", "/foo\r\nX-Inject: 1")` のような CRLF を含む値を構築できる
- 大文字を含む field-name (`HeaderField::from_str("Host", ...)`) を構築できる
- 全フィールドが `pub` のため、`header.name = vec![b'H', b'o', b's', b't']` のように
  構築後に不正値を代入できる
- `connection/mod.rs` の `concatenate_cookies()` が構造体リテラルで直接構築している
- `:method`, `:scheme`, `:path`, `:authority`, `:status`, `:protocol` の値構文検査は
  `src/validation.rs` の `validate_request_headers` / `validate_response_headers` で
  事後検査するが、構築点で検出できない → HTTP Response Splitting (CWE-113) のリスク

## 根拠

- RFC 9113 §8.2: field-name は HTTP/2 メッセージ構築時に lowercase に変換 MUST
- RFC 9113 §8.2.1: field-name に 0x41-0x5a (大文字 ASCII) を含めてはならない (MUST NOT)
- RFC 9113 §8.2.1: field-value に NUL (0x00), CR (0x0D), LF (0x0A) を含めてはならない (MUST NOT)
- RFC 9113 §8.2.1: field-value は ASCII SP (0x20) / HTAB (0x09) で開始・終了してはならない (MUST NOT)
- RFC 9113 §8.3: 疑似ヘッダー名は `:` で始まる定義済みの集合のみ許可 (§8.3.1: `:method`, `:scheme`, `:authority`, `:path` / §8.3.2: `:status`)。`:protocol` は RFC 8441 §4 による拡張
- RFC 9110 §5.1: field-name = token (§5.6.2: token = 1*tchar)
- shiguredo_http11 が `Request::new` / `Response::header` で構築時検査を採用しており、
  HTTP/1.1 系の同種バグ (CWE-113, CWE-444) を構築点で全て弾く設計に到達している。
  HTTP/2 は HPACK 圧縮で値の出所が追跡しにくく、構築時検査の価値は HTTP/1.1 以上に大きい

## スコープ

`shiguredo_http2` ルートクレート内の `HeaderField` 構築 API すべてを対象とする。

### `from_validated_parts` の実装順序

`HeaderField::from_validated_parts` は本 issue (0024) で実装する。decoder の書き換え
(`src/hpack/decoder.rs` の `HeaderField::new` / `new_sensitive` → `from_validated_parts`)
も 0024 内で完結させる。

issue 0030 は `HeaderField` 以外の構築時検査型 (`Setting`, `ClientStreamId`,
`WindowIncrement` 等) の `from_validated_parts` を統一的に導入する issue であり、
`HeaderField::from_validated_parts` の設計を先例として参照する。

### decoder の検証責務

- **HPACK decoder** (`src/hpack/decoder.rs`): HPACK 符号化の構造検証のみ。検証済みの
  name/value を `from_validated_parts` で `HeaderField` に変換する
- **field-name / field-value の文字検査**: `validate_request_headers` /
  `validate_response_headers` で実施する (HPACK decoder は wire 上のデータをそのまま展開する)

### フィールドの private 化

`HeaderField` の全フィールド (`name`, `value`, `sensitive`) を private にし、
以下のアクセサを提供する:

```rust
impl HeaderField {
    pub fn name(&self) -> &[u8];
    pub fn value(&self) -> &[u8];
    pub fn sensitive(&self) -> bool;
    pub fn size(&self) -> usize;  // 既存メソッド維持
}
```

これにより構造体リテラルでの直接構築や、構築後のフィールド書き換えを防止する。
影響を受けるファイルの詳細は「影響範囲」セクションを参照。

### API 変更

```rust
// 変更前
impl HeaderField {
    pub fn new(name: Vec<u8>, value: Vec<u8>) -> Self;
    pub fn new_sensitive(name: Vec<u8>, value: Vec<u8>, sensitive: bool) -> Self;
    pub fn from_str(name: &str, value: &str) -> Self;
    pub fn sensitive(name: &str, value: &str) -> Self;
}

// 変更後
impl HeaderField {
    /// ランタイム値から検査つきで構築する (sensitive: false)
    /// 内部で .as_ref().to_vec() するため、&str / &[u8] / Vec<u8> いずれも受け付ける。
    /// Vec<u8> を渡した場合はコピーが発生するが、ヘッダー構築はホットパスではないため許容する
    pub fn new(name: impl AsRef<[u8]>, value: impl AsRef<[u8]>)
        -> Result<Self, HeaderFieldError>;

    /// ランタイム値から検査つきで構築する (sensitive 指定可能)
    pub fn new_with_sensitive(
        name: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
        sensitive: bool,
    ) -> Result<Self, HeaderFieldError>;

    /// 静的バイト列から検査つきで構築する (const fn, sensitive: false)
    ///
    /// 不正なリテラルを渡すとコンパイル時に panic (= コンパイルエラー) になる。
    /// 検査内容は `new` と等価で、`const` コンテキストでの検査制約 (`Vec` 不可、`?` 不可)
    /// に合わせて実装する。
    pub const fn from_static(name: &'static [u8], value: &'static [u8]) -> Self;

    /// 検証済みバイト列から検査をスキップして構築する (crate 内部専用)
    /// sensitive フラグも受け取る (decoder の Never-Indexed 対応のため)
    pub(crate) fn from_validated_parts(name: Vec<u8>, value: Vec<u8>, sensitive: bool) -> Self;
}
```

### `const fn` の実装上の注意

- `from_static` 内部では `Vec<u8>` を作れないため、`HeaderField` の内部表現を
  `Cow<'static, [u8]>` あるいは `enum HeaderBytes { Static(&'static [u8]), Owned(Vec<u8>) }`
  のような表現に変更する必要がある。これは issue 0013 (Bytes 化) と統合検討する
- `const fn` 内で `Err` を返す機構 (`const Try`) は MSRV 1.88 では未安定。代わりに
  `panic!("field-name must be lowercase: ...")` で fail させる。const eval が panic を
  含むコードを評価するとコンパイルエラーになるため、利用者から見れば「不正リテラル =
  コンパイル不能」になる
- `const fn` 経由のエラーは構造化情報を失うため、ランタイム入力には引き続き
  `new` (`Result` 版) を使う

### 新規エラー型

`HeaderFieldError` は `Display` + `std::error::Error` を実装する。
`Clone` / `PartialEq` / `Eq` を derive する。`Vec<u8>` フィールドを持つため `Copy` は
導出不可能 (issue 0029 の「全て Copy」方針はこの型には適用できないため、0029 側を修正する)。

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderFieldError {
    /// field-name が空
    /// (RFC 9110 §5.1, §5.6.2: token = 1*tchar)
    EmptyFieldName,

    /// field-name に lowercase 以外の ASCII 英字が含まれる
    /// (RFC 9113 §8.2.1: MUST NOT contain 0x41-0x5a)
    UppercaseFieldName { name: Vec<u8> },

    /// field-name に token 文字以外が含まれる
    /// (RFC 9110 §5.1, §5.6.2 / RFC 9113 §8.2.1)
    InvalidFieldNameByte { name: Vec<u8>, byte: u8 },

    /// field-value に NUL/CR/LF が含まれる
    /// (RFC 9113 §8.2.1 MUST NOT)
    InvalidFieldValueByte { name: Vec<u8>, byte: u8 },

    /// field-value が先頭または末尾に SP/HTAB を含む
    /// (RFC 9113 §8.2.1 MUST NOT)
    FieldValueLeadingOrTrailingWhitespace { name: Vec<u8> },

    /// 疑似ヘッダー名が未定義 (`:foo` のような不明な疑似ヘッダー)
    /// (RFC 9113 §8.3, RFC 8441 §4)
    UnknownPseudoHeader { name: Vec<u8> },

    /// 疑似ヘッダー値が構文違反
    /// 各疑似ヘッダーの構文根拠:
    /// - `:method`: RFC 9110 §9.1 (token)
    /// - `:scheme`: RFC 3986 §3.1 (scheme)
    /// - `:path`: RFC 9113 §8.3.1, RFC 9110 §4.1 (absolute-path)
    /// - `:status`: RFC 9112 §4, RFC 9110 §15 (3DIGIT)
    /// - `:protocol`: RFC 8441 §4 (HTTP Upgrade Token)
    /// - `:authority`: RFC 3986 §3.2, RFC 9113 §8.3.1 (authority, userinfo 拒否)
    InvalidPseudoHeaderValue { name: Vec<u8>, value: Vec<u8> },
}
```

### 検査内容

`HeaderField::new` / `new_with_sensitive` で実施する検査:

1. field-name が空でないこと (RFC 9110 §5.1, §5.6.2: `token = 1*tchar`)
2. field-name が lowercase ASCII + token 文字のみであること (RFC 9113 §8.2.1, RFC 9110 §5.6.2)
3. field-value に NUL (0x00) / CR (0x0D) / LF (0x0A) を含まないこと (RFC 9113 §8.2.1)
4. field-value が SP (0x20) / HTAB (0x09) で開始・終了しないこと (RFC 9113 §8.2.1)
5. 疑似ヘッダー (`:` で始まる) の場合は、名前が `:method` / `:scheme` / `:authority` /
   `:path` / `:status` / `:protocol` のいずれかであることを確認
   (RFC 9113 §8.3.1, §8.3.2, RFC 8441 §4)
6. 疑似ヘッダーの値構文 (現在 `src/validation.rs` で実施している分のうち、
   単一フィールドで判定可能なものを構築時へ移動):
   - `:method`: token (RFC 9110 §9.1, §5.6.2)
   - `:scheme`: scheme 構文 (RFC 3986 §3.1)
   - `:path`: absolute-path (RFC 9110 §4.1) または `*` (asterisk-form)。
     空判定は scheme 依存 (http/https のみ MUST NOT empty) のため `validation.rs` 側に残す
   - `:status`: 3DIGIT (RFC 9112 §4)
   - `:protocol`: HTTP Upgrade Token (RFC 8441 §4)
   - `:authority`: authority 構文 (RFC 3986 §3.2)

以下の検査は「ヘッダーリスト全体」または「他のフィールドとの組み合わせ」に依存するため、
`src/validation.rs` 側に残す:

- リクエスト/レスポンスの整合性 (例: CONNECT に `:path` が無い等)
- `:path` の http/https scheme 依存の検査 (「パス不在なら `/` 必須」は `:scheme` の値を
  参照するため、単一フィールドの構築時検査では実施不能)
- `:authority` の userinfo 拒否 (RFC 9113 §8.3.1 は「http/https schemed URIs」に限定しており、
  `:scheme` の値を参照するため、単一フィールドの構築時検査では実施不能)
- OPTIONS リクエストの `*` 形式 (`:method` の値を参照するため)
- 疑似ヘッダーの順序・重複・存在チェック

### `ValidationError` との棲み分け

`src/validation.rs` の `ValidationError` から以下のバリアントを `HeaderFieldError` に移行する:

| 移行する (HeaderFieldError へ) | 残す (ValidationError に) |
|---|---|
| `InvalidHeaderName` | `MissingPseudoHeader` |
| `InvalidHeaderValue` | `DuplicatePseudoHeader` |
| `InvalidMethodValue` | `PseudoHeaderAfterRegular` |
| `InvalidSchemeValue` | `InvalidPseudoHeader` (注1: 分割) |
| `InvalidPathValue` | `ForbiddenHeader` |
| `InvalidStatusCode` | `InvalidTeHeader` |
| `InvalidProtocolValue` | `EmptyPath` (scheme 依存のため残す) |
| `InvalidPseudoHeader` (未知名の用途) → `UnknownPseudoHeader` | |
| | `AsteriskPathOnNonOptions` |
| | `ConnectWithPathOrScheme` |
| | `ConnectInvalidAuthority` |
| | `NonConnectMissingPathOrScheme` |
| | `ExtendedConnectMissingSchemeOrPath` |
| | `ProtocolOnNonConnect` |
| | `HostAuthorityMismatch` |
| | `MissingAuthority` |
| | `AuthorityWithUserinfo` |

`ValidationError` に残すものはヘッダーリスト全体の整合性検査に関するバリアントのみ。

注1: `InvalidPseudoHeader` は現在 2 つの用途で使われている:
- 未知の疑似ヘッダー名 (`:foo` 等) → `HeaderFieldError::UnknownPseudoHeader` に移行
- トレーラーに疑似ヘッダーが含まれる (`validate_trailers`) → リスト整合性検査のため
  `ValidationError` に残す。ただし名称を `PseudoHeaderInTrailers` 等に変更して
  `UnknownPseudoHeader` との混同を避ける

## 影響範囲

- `src/hpack/table.rs`: `HeaderField` フィールド private 化、アクセサ追加、コンストラクタ変更、
  `StaticEntry::to_header_field()` を `from_validated_parts` 経由に変更
- `src/hpack/mod.rs`: `HeaderFieldError` の re-export
- `src/hpack/decoder.rs`: `from_validated_parts` 経由で構築するよう書き換え
- `src/hpack/encoder.rs`: フィールドアクセスをアクセサ経由に変更
- `src/hpack/dynamic_table.rs`: `insert()` 内の `HeaderField::new` を `from_validated_parts` に変更
- `src/connection/mod.rs`: `concatenate_cookies()` を `from_validated_parts` 経由に変更、
  `start_stream` / `send_response` / `send_trailers` の内部値検査を削減
- `src/validation.rs`: 個別フィールド値検査を `HeaderField::new` に移し、リスト整合性のみを残す。
  `ValidationError` から移行対象バリアントを削除
- `src/event.rs`: フィールドアクセスをアクセサ経由に変更
- `src/lib.rs`: `HeaderFieldError` の `pub use` 追加
- `tests/rfc7541.rs`: フィールドアクセスと構築方法の書き換え
- `pbt/tests/prop_hpack.rs`: `new_sensitive` → `new_with_sensitive` の書き換え、フィールドアクセス変更
- `pbt/tests/prop_validation.rs`: 構築時検査の導入に伴いテスト戦略の見直しが必要 (詳細は issue 0031)
- `fuzz/fuzz_targets/`: 新 API への書き換え
- `examples/http2_client/`, `examples/http2_server/`: `from_str` → `new` + `?` への書き換え、
  フィールドアクセスをアクセサ経由に変更

## CHANGES.md エントリ

```
- [ADD] `HeaderField::from_static` を追加し、リテラル定数の RFC 違反をコンパイル時に
  検出可能にする
  - @担当者
- [CHANGE] `HeaderField::new` / `from_str` / `new_sensitive` / `sensitive` を廃止し、
  構築時検査つきの `HeaderField::new` (Result 型) と `HeaderField::new_with_sensitive` に
  置き換える
  - @担当者
- [CHANGE] `HeaderField` のフィールドを private 化し、アクセサメソッドを提供する
  - @担当者
```

## 受け入れ条件

- `HeaderField::from_str` / `new` (旧シグネチャ) / `new_sensitive` / `sensitive` が削除されている
- `HeaderField::new` が `Result<Self, HeaderFieldError>` を返す
- `HeaderField::new_with_sensitive` が `sensitive` フラグを受け取り `Result<Self, HeaderFieldError>` を返す
- `HeaderField::from_static` が `const fn` で実装され、不正リテラルでコンパイルエラーになる
- `from_static` のコンパイル時失敗ケースを `trybuild` で担保する (issue 0032)
- `HeaderField` の全フィールドが private で、アクセサ経由でのみ読み取れる
- decoder 経路は `from_validated_parts` 経由で構築している
- `from_validated_parts` が `sensitive` フラグを受け取る
- `StaticEntry::to_header_field()` が `from_validated_parts` 経由で構築している
- `DynamicTable::insert()` が `from_validated_parts` 経由で構築している
- `concatenate_cookies()` が `from_validated_parts` 経由で構築している
- `HeaderFieldError` が `Display` + `std::error::Error` を実装している
- `src/validation.rs` の個別フィールド値検査が `HeaderField::new` に統合され、
  リスト整合性検査のみが残っている
- `ValidationError` から移行対象バリアントが削除されている
- 既存の全テスト・PBT・fuzz が通る

## 依存

- [[0013-refactor-bytes-payloads]] (`from_static` の内部表現変更に必要)
- [[0029-change-split-error-types]] (`HeaderFieldError` の設計方針。0029 側で `Copy` 不可の型を許容するよう修正が必要)

## 関連

- [[0025-change-stream-id-newtype]] (StreamId NewType 化)
- [[0030-add-from-validated-parts-internal-constructors]] (他型の `from_validated_parts` 統一。0030 側の `HeaderField::from_validated_parts` シグネチャを `sensitive: bool` 追加に修正する必要あり)
- [[0031-add-pbt-construct-time-validation]] (PBT 整備)
- [[0032-add-trybuild-compile-fail-tests]] (`from_static` の compile_fail テスト)
