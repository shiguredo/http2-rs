# HeaderField を構築時検査型に変更する

Created: 2026-05-23
Model: Opus 4.7

## 概要

`HeaderField` を「不正な値を持てない型」に作り直す。現状の `HeaderField::from_str(name, value)` /
`HeaderField::from_bytes(name, value)` は無検査でフィールドを構築できるため、不正な値を持った
`HeaderField` を上位レイヤや送信パスまで持ち回せてしまう。

破壊的変更を伴う。`shiguredo_http2` の公開 API のうち `HeaderField` 構築系をすべて
`Result<Self, HeaderFieldError>` 化し、加えてリテラル定数向けの `const fn` 構築 API
(`HeaderField::from_static`) を提供して **RFC 違反をコンパイル時に検出可能**にする。

## 設計方針: 二段の構築 API

利用者の入力源によって 2 つの構築点を提供する。

| API | 対象 | 検査タイミング | 失敗時の挙動 |
|---|---|---|---|
| `HeaderField::new(name, value)` | ランタイム値 (`&str` / `&[u8]` / `Vec<u8>`) | 実行時 | `Err(HeaderFieldError)` |
| `HeaderField::from_static(name, value)` | `&'static [u8]` リテラル | コンパイル時 (`const fn`) | コンパイルエラー (const eval panic) |

`from_static` を `const fn` で実装することで、以下のような定数定義はリテラルが
RFC 違反なら **CI を回す前にコンパイルエラー**で検出される。

```rust
// OK: コンパイル成功
const METHOD: HeaderField = HeaderField::from_static(b":method", b"GET");

// NG: コンパイル時に "field-name must be lowercase" で fail
const BAD: HeaderField = HeaderField::from_static(b"Host", b"example.com");

// NG: コンパイル時に "field-value contains CR" で fail
const INJECT: HeaderField = HeaderField::from_static(b":path", b"/foo\r\nX-Inject: 1");
```

これは hyper/h2 等の他の HTTP/2 実装には無い特徴であり、本ライブラリの差別化要素となる。

## 背景

現状のコードでは以下の構築点を通った後の `HeaderField` の検査責務が散在している:

- `HeaderField::from_str(":path", "/foo\r\nX-Inject: 1")` のような CRLF を含む値を構築できる
- 大文字を含む field-name (`HeaderField::from_str("Host", ...)`) を構築できる
- `:method`, `:scheme`, `:path`, `:authority`, `:status`, `:protocol` の値構文検査は
  `src/validation.rs` の `validate_request_headers` / `validate_response_headers` で
  事後検査するが、ここに到達する前に構築点が複数あり、検査前の値で `Vec<HeaderField>` を
  操作するコードが存在する

結果として、以下のリスクが残る:

- HTTP Response Splitting (CWE-113): CRLF を含むヘッダー値が後段のエンコーダで素通しされる経路
- HPACK 経由の field-name 大文字検査漏れ (RFC 9113 §8.2.1: lowercase MUST)
- 疑似ヘッダー値の構文違反 (RFC 9113 §8.3 / RFC 3986 / RFC 7230) を構築点で検出できない

## 根拠

- RFC 9113 §8.2.1: field-name は HTTP/2 では lowercase ASCII MUST。違反は PROTOCOL_ERROR
- RFC 9113 §8.2.1: field-value に NUL (0x00), CR (0x0D), LF (0x0A) を含めてはならない (MUST NOT)
- RFC 9113 §8.3: 疑似ヘッダー名は `:` で始まる定義済みの集合のみ許可
- shiguredo_http11 が `Request::new` / `Response::header` で構築時検査を採用しており、
  HTTP/1.1 系の同種バグ (CWE-113, CWE-444) を構築点で全て弾く設計に到達している。
  HTTP/2 は HPACK 圧縮で値の出所が追跡しにくく、構築時検査の価値は HTTP/1.1 以上に大きい

## スコープ

`shiguredo_http2` ルートクレート内の `HeaderField` 構築 API すべてを対象とする。
内部で decoder が検証済みバイト列から `HeaderField` を再構築する経路は二重検査を避けるため
`pub(crate) from_validated_parts` を別途用意する (別 issue 0030 で扱う)。

### API 変更

```rust
// 変更前
impl HeaderField {
    pub fn from_str(name: &str, value: &str) -> Self;
    pub fn from_bytes(name: Vec<u8>, value: Vec<u8>) -> Self;
}

// 変更後
impl HeaderField {
    /// ランタイム値から検査つきで構築する
    pub fn new(name: impl AsRef<[u8]>, value: impl AsRef<[u8]>)
        -> Result<Self, HeaderFieldError>;

    /// 静的バイト列から検査つきで構築する (const fn)
    ///
    /// 不正なリテラルを渡すとコンパイル時に panic (= コンパイルエラー) になる。
    /// 検査内容は `new` と等価で、`const` コンテキストでの検査制約 (`Vec` 不可、`?` 不可)
    /// に合わせて実装する。
    pub const fn from_static(name: &'static [u8], value: &'static [u8]) -> Self;

    /// 検証済みバイト列から検査をスキップして構築する (decoder 専用)
    pub(crate) fn from_validated_parts(name: Vec<u8>, value: Vec<u8>) -> Self;
}
```

### `const fn` の実装上の注意

- `from_static` 内部では `Vec<u8>` を作れないため、`HeaderField` の内部表現を
  `Cow<'static, [u8]>` あるいは `enum HeaderBytes { Static(&'static [u8]), Owned(Vec<u8>) }`
  のような表現に変更する必要がある。これは別 issue (バイト表現変更) と統合検討する
- `const fn` 内で `Err` を返す機構 (`const Try`) は MSRV 1.88 では未安定。代わりに
  `panic!("field-name must be lowercase: ...")` で fail させる。const eval が panic を
  含むコードを評価するとコンパイルエラーになるため、利用者から見れば「不正リテラル =
  コンパイル不能」になる
- `const fn` 経由のエラーは構造化情報を失うため、ランタイム入力には引き続き
  `new` (`Result` 版) を使う

### 新規エラー型

```rust
#[non_exhaustive]
pub enum HeaderFieldError {
    /// field-name に lowercase 以外の ASCII 英字が含まれる
    /// (RFC 9113 §8.2.1)
    UppercaseFieldName { name: Vec<u8> },

    /// field-name に token 文字以外が含まれる
    /// (RFC 9110 §5.1 / RFC 9113 §8.2.1)
    InvalidFieldNameByte { name: Vec<u8>, byte: u8 },

    /// field-name が空
    EmptyFieldName,

    /// field-value に NUL/CR/LF が含まれる
    /// (RFC 9113 §8.2.1 MUST NOT)
    InvalidFieldValueByte { name: Vec<u8>, byte: u8 },

    /// field-value が field-vchar 規則に違反 (前後の OWS, etc.)
    /// (RFC 9110 §5.5)
    InvalidFieldValueShape { name: Vec<u8>, value: Vec<u8> },

    /// 疑似ヘッダー名が未定義 (`:foo` のような不明な疑似ヘッダー)
    /// (RFC 9113 §8.3)
    UnknownPseudoHeader { name: Vec<u8> },

    /// 疑似ヘッダー値が構文違反
    /// (RFC 9113 §8.3.1 リクエスト / §8.3.2 レスポンス)
    InvalidPseudoHeaderValue { name: Vec<u8>, value: Vec<u8> },
}
```

### 検査内容

`HeaderField::new` で実施する検査:

1. field-name の空判定
2. field-name の lowercase ASCII + token 文字判定
3. field-value の NUL/CR/LF 拒否
4. 疑似ヘッダー (`:` で始まる) の場合は、名前が `:method` / `:scheme` / `:authority` /
   `:path` / `:status` / `:protocol` のいずれかであることを確認
5. 疑似ヘッダーの値構文 (現在 `src/validation.rs` で実施している分を構築時へ移動):
   - `:method`: token (RFC 9110 §9.1)
   - `:scheme`: scheme 構文 (RFC 3986 §3.1)
   - `:path`: 空でない (CONNECT 以外), absolute-path or `*` (OPTIONS 限定)
   - `:status`: 3 桁数字 (RFC 9110 §15)
   - `:protocol`: token (RFC 8441)
   - `:authority`: authority 構文 (userinfo 拒否)

「リクエスト/レスポンスの整合性」(例: CONNECT に :path が無い等) は構築単位ではなく
ヘッダーリスト全体に対する検査なので、`src/validation.rs` 側に残す。

## 影響範囲

- `src/hpack/mod.rs`: `HeaderField` 定義と公開 API
- `src/hpack/decoder.rs`: `from_validated_parts` 経由で構築するよう書き換え
- `src/hpack/encoder.rs`: 検証済みの `HeaderField` を前提とした記述に整理
- `src/connection/mod.rs`: `start_stream` / `send_response` / `send_trailers` で受け取る
  `Vec<HeaderField>` は構築時検査済みのため、内部の値検査を削減
- `src/validation.rs`: 個別フィールド値検査を `HeaderField::new` に移し、リスト整合性のみを残す
- `tests/`, `pbt/tests/`, `fuzz/fuzz_targets/`: 新 API への書き換え
- `examples/*`: 全て `?` 経由の書き換え

## CHANGES.md エントリ

```
- [CHANGE] `HeaderField::from_str` / `from_bytes` を廃止し、構築時検査つきの
  `HeaderField::new` (Result 型) と `HeaderField::from_static` (const fn) に置き換える
- [ADD] `HeaderField::from_static` を追加し、リテラル定数の RFC 違反をコンパイル時に
  検出可能にする
```

## 受け入れ条件

- `HeaderField::from_str` / `from_bytes` が削除されている
- `HeaderField::new` が `Result<Self, HeaderFieldError>` を返す
- `HeaderField::from_static` が `const fn` で実装され、不正リテラルでコンパイルエラーになる
- `from_static` のコンパイル時失敗ケースを `compile_fail` の doctest または
  `trybuild` 相当の仕組みで担保する
- decoder 経路は `from_validated_parts` 経由で構築している
- `HeaderField::new` で弾かれる入力 (CRLF/NUL/uppercase/不正疑似ヘッダー値) の
  単体テストが揃っている
- PBT で「`HeaderField::new(name, value)` が成功した値は encoder→decoder で同値を返す」を検証
- `src/validation.rs` の個別フィールド値検査が `HeaderField::new` に統合され、
  リスト整合性検査のみが残っている
- 既存の全テスト・PBT・fuzz が通る

## 関連

- 親議論: 構築時検査による堅牢化方針
- [[0025-change-stream-id-newtype]] (StreamId NewType 化)
- [[0029-change-split-error-types]] (エラー型細分化)
- [[0030-add-from-validated-parts-internal-constructors]] (内部用コンストラクタ)
- [[0031-add-pbt-construct-time-validation]] (PBT 整備)
