# WT-Available-Protocols / WT-Protocol サブプロトコルネゴシエーションを追加する

- Priority: Medium
- Created: 2026-06-08
- Polished: 2026-06-15
- Model: deepseek-v4-pro
- Branch: feature/add-wt-subprotocol-negotiation

## 目的

draft-ietf-webtrans-http2-14 Section 3.3 に定義されているサブプロトコルネゴシエーション機能 (WT-Available-Protocols リクエストヘッダー / WT-Protocol レスポンスヘッダー) をサーバー側に実装し、ALPN ライクなサブプロトコル選択を可能にする。

## 優先度根拠

仕様上の MAY 要件 (サブプロトコルは必須ではない)。既存プロトコルの WebTransport 移植に有用だが必須ではないため Priority は Medium。

## 現状

draft-ietf-webtrans-http2-14 Section 3.3 (L317-L342):

> The user agent MAY include a WT-Available-Protocols header field in the CONNECT request.  The WT-Available-Protocols enumerates the possible protocols in preference order.  If the server receives such a header, it MAY include a WT-Protocol field in a successful (2xx) response.  If it does, the server MUST include a single choice from the client's list in that field.  Servers MAY reject the request if the client did not include a suitable protocol.
>
> Both WT-Available-Protocols and WT-Protocol are defined in Section 3.4 of [WEBTRANSPORT-H3].


draft-ietf-webtrans-http3-15 Section 3.3 (`refs/draft-ietf-webtrans-http3-15.txt` L531-L585) の主要要件:

- `WT-Available-Protocols` は **RFC 8941 List of String** (preference order、最優先が先頭) (L547-L549)
- `WT-Protocol` は **RFC 8941 Item of String** (L547-L549)
- 値型は String のみ。String 以外の値型は MUST「field 全体を無視」 (L550-L551 「Any value type other than String MUST be treated as an error that causes the entire field to be ignored」)
- パラメータには意味は定義されない。MUST「パラメータは無視」 (L553-L554 「No semantics are defined for parameters on either field; parameters MUST be ignored」)
- サーバーは WT-Available-Protocols が不在・不正な場合にセッションを拒否してもよい (L565-L567 「A server that requires application protocol negotiation MAY reject the session if the WT-Available-Protocols header field is absent or if it is malformed and therefore ignored」)
- サーバーは WT-Protocol の値を WT-Available-Protocols に含まれる 1 つにしなければならない (L543 「the server MUST include a single choice from the client's list in that field」)。この制約を満たさない selected_protocol を指定した場合、サーバーは CONNECT ストリームをエラーにする
- クライアント側でも、受信した WT-Protocol が WT-Available-Protocols に含まれない場合は WT_ALPN_ERROR でセッションを閉じる MUST (L574-L580)。本 issue の実装対象はサーバー側の selected_protocol 含有検証である

現在の実装: `grep -rn "wt-available-protocols\|wt-protocol\|selected_protocol" src/ crates/` でヒットなし、完全な未実装。

## 本 issue で扱う

- Sans I/O 層: `WtAvailableProtocols` 構造体 (preference order を保持する `Vec<String>` ラッパ) と `WtAvailableProtocols::parse(value: &[u8]) -> Result<WtAvailableProtocols, WtError>` を `src/webtransport/protocols.rs` (新規) に追加。`serialize_wt_protocol(value: &[u8]) -> Result<Vec<u8>, WtError>` (sf-string シリアライズ) も同モジュールに追加
- tokio-http2 層:
  - `WtServerRequest::wt_available_protocols(&self) -> Option<&[u8]>` (生バイト列 helper) を追加
  - `WtServerRequest::accept()` のシグネチャに `selected_protocol: Option<&[u8]>` 引数を追加
  - `accept()` 内で 0063 設計判断 5 の「TLS → Origin → 0064 Init → `:status=200` 送信」という大枠に従い、サブプロトコル検証と WT-Protocol レスポンスヘッダー追加を `:status=200` 送信の直前に配置する
  - `WtServerSession` / `WtSessionParts` に `selected_protocol: Option<Vec<u8>>` フィールドを追加し、`WtServerSession::selected_protocol(&self) -> Option<&[u8]>` を公開

## 本 issue のスコープ外

- **クライアント側の WT-Available-Protocols 送信 / WT-Protocol 受信検証**: 現状 `crates/tokio-http2/src/client.rs` に WebTransport クライアント API が無いため対象外。クライアント側 WebTransport API 追加時に併せて対応する
- **`sfv` クレートなど外部依存の追加**: 0064 と同じく依存最小化方針 (CLAUDE.md / shiguredo-rust 規約) に従い必要最小限の自前パーサーで対応する
- **suitable protocol が無い場合の `MAY reject`**: 仕様 (L565-L567) は MAY なので、本実装ではサーバー側の自動拒否は行わない。呼び出し側が `wt_available_protocols()` を見て不適合と判断した場合、明示的に `reject(406)` を呼ぶ (`reject` は既存 API)
- **複数 `wt-available-protocols` ヘッダーの結合**: 0064 と同様、最初の 1 個のみを評価する。複数行結合は将来必要になれば別 issue で対応する

## 設計判断

### 1. RFC 8941 List of String / Item of String パーサーは Sans I/O 層に新規モジュール (`src/webtransport/protocols.rs`) を作る

0064 は Dictionary + Integer の最小パーサーを `src/webtransport/init.rs` に置くが、本 issue は List + Item + String の別ロジックを必要とする (Dictionary key 抽出と List 要素抽出は異なる)。責務を分けるため別モジュールに置く。共通化は両方が安定してから別 issue で検討する。

### 2. パース失敗時の挙動: 「field を無視」(仕様準拠)

仕様 L550-L551 「Any value type other than String MUST be treated as an error that causes the entire field to be ignored」は String 以外の値型を ignore せよと規定する。これは 0064 の WebTransport-Init における「parse error で 4xx を返す」挙動とは異なる。挙動差の根拠は両仕様の MUST 条文の違いに由来する: 0064 (WebTransport-Init) は draft-ietf-webtrans-http2-14 Section 4.3 が 4xx 拒否を要求するのに対し、本 issue (WT-Available-Protocols / WT-Protocol) は draft-ietf-webtrans-http3-15 Section 3.3 L550-L551 が ignore を要求する。

`WtAvailableProtocols::parse` は型不一致 (String 以外の値) およびパース不能 (不正バイト・未終端 DQUOTE・不正エスケープ等) の場合に `WtError::invalid_input` を返す。`accept()` 内では `Result::ok()` で `Option` に畳み込み、エラー時は「ヘッダー不在」と等価に扱う (= サブプロトコルなし)。**4xx は返さない**。

空入力 `b""` の扱い: RFC 8941 Section 3.1 (`refs/rfc8941.txt` L324) は「Lists are arrays of zero or more members」と定義し、L337-L338 で「An empty List is denoted by not serializing the field at all. This implies that fields defined as Lists have a default empty value.」と明示している。さらに Section 4.2.1 step 3 (L1112-L1113) は「`No structured data has been found; return members (which is empty).`」と規定するため、空入力は parse error ではなく empty list を返すのが仕様準拠。本実装も `WtAvailableProtocols::parse(b"") = Ok(WtAvailableProtocols { protocols: vec![] })` とし、空リスト時の挙動は設計判断 4 (`selected_protocol == Some(_)` で含有検証失敗) に従う。

設計判断 2 と設計判断 4 の境界整理:
- `selected_protocol == None`: WT-Available-Protocols が不在・パース失敗・空リストのいずれでも正常受理 (4xx も RST_STREAM も発生しない)
- `selected_protocol == Some(p)`: クライアントリストの取得可否で分岐
  - ヘッダー不在 / パース失敗 (= ignore) / 空リスト / `p` がリストに含まれない / `p` のシリアライズ失敗のいずれか → 設計判断 4 に従い `RST_STREAM` + `Error::InvalidArgument`
  - `p` が含まれる → 正常受理 + WT-Protocol レスポンスヘッダー付与

### 3. WT-Protocol 値の型は `Option<&[u8]>` で API 統一

`accept()` の引数は既存の `allowed_origin: Option<&[u8]>` と統一するため `selected_protocol: Option<&[u8]>` を採用する。RFC 8941 sf-string は ASCII printable (0x20-0x7E) のみなので、シリアライズ時に `serialize_wt_protocol()` が検証する。`accept()` 側では追加検証を行わず、`serialize_wt_protocol()` の結果を `Error::InvalidArgument` にマップする。

戻り値型は `WtServerSession::selected_protocol(&self) -> Option<&[u8]>` で統一。

### 4. selected_protocol のクライアントリスト含有検証とシリアライズ失敗時の挙動

draft-ietf-webtrans-http3-15 L543 「the server MUST include a single choice from the client's list」に従い、`selected_protocol` が `Some(_)` の場合は WT-Available-Protocols をパースして含有検証を行う。以下のいずれも呼び出し側 (サーバーアプリケーション) のミスとして `Error::InvalidArgument` を返し、**レスポンス (':status=200' 含む) は一切送信せず CONNECT ストリームを `RST_STREAM(ErrorCode::InternalError)` で閉じる**:

- クライアントリスト不在 (WT-Available-Protocols ヘッダーがない、または設計判断 2 によりパース失敗で ignore されている)
- クライアントリストが空 (空入力でパース成功するが要素 0)
- クライアントリストに `selected_protocol` が含まれていない
- `selected_protocol` のシリアライズに失敗した (ASCII printable 外のバイト等)

RST_STREAM の error_code は `ErrorCode::InternalError` (RFC 9113 §7) を使う。`ProtocolError` は「対向の HTTP/2 規約違反」を意味するため不適切 — 本ケースはサーバー側 API 利用者のロジック誤りであり、対向のクライアント要求自体は正当である。`InternalError` は「予期しないサーバー内部条件で要求を処理できない」という意味で適合する。

これにより、TLS バージョン未達時 (0063) の `RST_STREAM` 処理と挙動を揃え、ストリームが宙吊りになるのを防ぐ (0063 でも RST_STREAM の error_code は実装の方針に従う)。

なお、パース失敗 (= ignore) の詳細エラー情報はサーバーアプリケーション側で観測できないが、これは仕様 L550-L551 が「ignore」と要求しているため避けられない。アプリケーションが事前にパースエラーを観測したい場合は `WtServerRequest::wt_available_protocols()` の生バイト列を取得して独自に `WtAvailableProtocols::parse()` を呼ぶことで対応できる (Sans I/O 層の `parse` は `pub` 公開済み)。

### 5. accept() シグネチャ拡張は破壊的変更で CHANGES.md は `[CHANGE]`

`accept(mut self, config, allowed_origin)` → `accept(mut self, config, allowed_origin, selected_protocol)` の引数追加。既存呼び出し側 (`crates/tokio-http2/tests/test_webtransport.rs` 16 箇所、`examples/wt_server` 1 箇所) は全件更新が必要。CHANGES.md `## develop` セクションに `[CHANGE]` エントリ (シグネチャ拡張 + WT-Available-Protocols / WT-Protocol サポート追加を 1 エントリにまとめる) を追加する。`shiguredo-issues` 規約 (最近の方針) により、CHANGES.md エントリには issue 番号を含めない。なお本 issue のファイル名は `0066-add-` だが、シグネチャ拡張は破壊的なため CHANGES.md は `[ADD]` ではなく `[CHANGE]` を採用する。

### 6. WtServerSession / WtSessionParts への selected_protocol フィールド追加

`WtServerSession::selected_protocol()` が `&[u8]` を返すには構造体にフィールドが必要。`accept()` が確定値を `Option<Vec<u8>>` で持ち、`into_parts()` で `WtSessionParts` にも同フィールドを伝播する。driver タスクは selected_protocol を扱わない (TLS exporter のようなランタイム参照は不要)。

伝播の所有権ルール:
- `into_parts(self)` は self を消費するため、`WtServerSession` 自体が move される。よって `selected_protocol: Option<Vec<u8>>` フィールドは `into_parts` で move して `WtSessionParts` に渡せばよく、move 後の `WtServerSession::selected_protocol()` 呼び出しは型システム上不可能 (コンパイラがエラーにする)
- `WtSessionParts` 側にもアクセサ `pub fn selected_protocol(&self) -> Option<&[u8]>` を公開する。これにより `into_parts` 前後で同じ API パターンを保てる

選択肢として `Arc<Option<Vec<u8>>>` で共有する案もあるが、`into_parts` が self を消費する現状設計と整合しないため採用しない。

`Vec<String>` を採用する根拠 (`WtAvailableProtocols::protocols` の型):
- RFC 8941 Section 3.3.3 (`refs/rfc8941.txt` L583-L584) で sf-string の値域は ASCII 0x20-0x7E に限定される
- Rust の `String` (内部 UTF-8) はその subset である ASCII を安全に保持できる
- 比較は `protocols.iter().any(|p| p.as_bytes() == protocol_bytes)` のように `as_bytes()` でバイト一致を取れば変換コストはほぼゼロ
- 将来 sf-string の値域が拡張された場合に `Vec<Vec<u8>>` へ切り替える余地は残す

### 7. レスポンスの WT-Protocol シリアライズ

sf-string の ABNF 定義 (RFC 8941 §3.3.3 L589-L591):

```
sf-string = DQUOTE *chr DQUOTE
chr       = unescaped / escaped
unescaped = %x20-21 / %x23-5B / %x5D-7E
escaped   = "\" ( DQUOTE / "\" )
```

シリアライズ手順は RFC 8941 §4.1.6 (L924-L939) に従う。`serialize_wt_protocol(value: &[u8])` は値を DQUOTE で囲み、`"` と `\` を `\` でエスケープする。これにより `selected_protocol = b"echo"` は wire 上で `"echo"` (DQUOTE 込み 6 バイト) となる。値域外バイト (0x00-0x1F または 0x7F-0xFF、RFC 8941 §4.1.6 step 2 で禁止) は `WtError::invalid_input` を返す。

空入力 `b""` は ABNF 上は `DQUOTE *chr DQUOTE` の `*chr` が 0 回マッチで許容されるが、サブプロトコル名として意味を持たないため、本実装では `selected_protocol = Some(b"")` のとき `Error::InvalidArgument` で早期に弾く (設計判断 4 のシリアライズ失敗扱い)。

`HeaderField::new(b"wt-protocol", &serialized).expect("serialized wt-protocol is a valid header value")` でレスポンスヘッダーリストに追加する。`HeaderField::from_static` は使えない (実行時値のため)。`expect` の根拠: `serialize_wt_protocol` は 0x20-0x7E のみ出力 (DQUOTE 包み + `\` エスケープ) するため、HTTP/2 `validate_field_value` の禁止バイト (0x00 / 0x0D / 0x0A) と衝突せず、かつ先頭/末尾は常に DQUOTE のため OWS にもならない。よって `HeaderField::new` は失敗しない。

### 8. チェック順序

0063 設計判断 5 で確定した順序に従う:

1. TLS バージョンチェック (0063)
2. Origin 検証 (0062)
3. WebTransport-Init パース・マージ (0064)
4. **WT-Available-Protocols パース + selected_protocol 検証 (本 issue)**
5. `:status=200` 送信 (WT-Protocol を含める) と `WtSession` 生成

部分ムーブ前に `&self` 借用が必要な値 (origin / init_bytes / available_bytes) はすべて取得しておき、`let Self { mut conn, stream_id, .. } = self;` で部分ムーブする。

### 9. エラー変換は既存の `wt_err` パターンを踏襲し、0077 で統合する

`WtAvailableProtocols::parse` / `serialize_wt_protocol` が返す `WtError::invalid_input` は、現行の `wt_err` 関数 (`crates/tokio-http2/src/webtransport.rs` L1053-L1055) により `Error::InvalidArgument` に変換する。open issue 0077 で `Error::WebTransport(WtError)` バリアントが導入され `wt_err` が除去される予定だが、本 issue では既存パターンを踏襲し、0077 の対応時に一括で移行する。本 issue 実装後、`accept()` 内の `.map_err(wt_err)` は 1 箇所増えるため、0077 実装時の置換対象数に注意する。

## 完了条件

- `src/webtransport/protocols.rs` (新規) に以下が追加されていること:
  - `pub struct WtAvailableProtocols { pub protocols: Vec<String> }`
  - `WtAvailableProtocols::parse(value: &[u8]) -> Result<WtAvailableProtocols, WtError>`
  - `pub fn serialize_wt_protocol(value: &[u8]) -> Result<Vec<u8>, WtError>` (sf-string シリアライズ)
- `src/webtransport/mod.rs` に `pub mod protocols;` と `pub use protocols::{WtAvailableProtocols, serialize_wt_protocol};` が追加されていること
- `tests/test_webtransport/main.rs` に `mod protocols;` が追加されていること
- `WtAvailableProtocols::parse` が以下を満たすこと:
  - 正常系: `"echo", "raw"` で `protocols = ["echo", "raw"]` (preference order = 入力順を保持)
  - 単一エントリ: `"echo"` で `protocols = ["echo"]`
  - パラメータ無視: `"echo";version=1, "raw";v=2` で `protocols = ["echo", "raw"]`
  - DQUOTE エスケープ解除: 入力 `"hello\"world"` を `hello"world` (0x68 0x65 0x6C 0x6C 0x6F 0x22 0x77 0x6F 0x72 0x6C 0x64) にパースする
  - 重複 (sf-list は重複可): `"a", "a", "b"` で `protocols = ["a", "a", "b"]`
  - 空入力 `b""` は `Ok(WtAvailableProtocols { protocols: vec![] })` を返す (RFC 8941 §3.1 + §4.2.1 step 3 準拠)
  - String 以外の値型 (`echo` (Token)、`123` (Integer)、`?1` (Boolean)、`:YWJj:` (Byte Sequence)、`(a b)` (Inner List)): `WtError::invalid_input` を返す
  - 不正バイト (0x80-0xFF) を含む入力: `WtError::invalid_input` (RFC 8941 §4.2 step 1)
- `serialize_wt_protocol` が以下を満たすこと:
  - 正常系: `b"echo"` → `b"\"echo\""` (DQUOTE 込み)
  - エスケープ: `b"a\"b"` → `b"\"a\\\"b\""` (`"` を `\"` に、`\` を `\\` に)
  - 値域外バイト (0x00-0x1F、0x7F-0xFF): `WtError::invalid_input`
  - 空入力 `b""`: `WtError::invalid_input` (サブプロトコル名として意味を持たないため、本実装では弾く)
- `WtServerRequest::wt_available_protocols(&self) -> Option<&[u8]>` が追加され、`b"wt-available-protocols"` (HTTP/2 lowercase) と一致するヘッダーの値を返すこと。doc コメントに「既知の制限: 同名ヘッダーが複数あった場合は最初の 1 個のみ」を明記する (0064 の `webtransport_init()` と整合)
- `WtServerRequest::accept(mut self, mut config: WtConfig, allowed_origin: Option<&[u8]>, selected_protocol: Option<&[u8]>) -> Result<WtServerSession>` シグネチャに変更されていること
- `accept()` が以下を満たすこと:
  - `selected_protocol == None` の場合: WT-Available-Protocols の有無・パース失敗・空リストに関わらず正常受理。WT-Protocol レスポンスヘッダーは付与しない
  - `selected_protocol == Some(p)` かつ WT-Available-Protocols がパース可能で `p` が含まれる場合: 正常受理。`:status=200` レスポンスに `wt-protocol: "p"` (sf-string serialized) を追加
  - `selected_protocol == Some(p)` かつ WT-Available-Protocols 不在 / パース失敗 (ignore) / 空リスト / `p` がリストに含まれない / `p` のシリアライズ失敗のいずれかの場合: `RST_STREAM(ErrorCode::InternalError)` を送信した上で `Error::InvalidArgument` を返す
- `WtServerSession` 構造体 (`webtransport.rs` L274-L280) に `selected_protocol: Option<Vec<u8>>` フィールドが追加され、`pub fn selected_protocol(&self) -> Option<&[u8]>` が公開されていること
- `WtSessionParts` (`webtransport.rs` L382-L395) にも `selected_protocol: Option<Vec<u8>>` フィールドが追加され、`into_parts()` で move 伝播すること。`WtSessionParts::selected_protocol(&self) -> Option<&[u8]>` アクセサも公開する
- 既存の `WtServerRequest::accept()` 呼び出し側 (`crates/tokio-http2/tests/test_webtransport.rs` の 16 箇所、`examples/wt_server` の 1 箇所) がすべて `selected_protocol` 引数 (`None` または明示値) を追加して更新されていること。書き換えパターンは「`.accept(WtConfig::default(), <allowed_origin>)` の末尾 `)` 直前に `, None` (または明示的なサブプロトコル) を挿入」する
- 単体テスト・統合テストが以下を検証していること:
  - Sans I/O 単体: 上記 `WtAvailableProtocols::parse` / `serialize_wt_protocol` の各境界ケース
  - 統合: WT-Available-Protocols が `"echo"` で `selected_protocol = Some(b"echo")` → 成功し、クライアントが WT-Protocol = `"echo"` を受信
  - 統合: selected_protocol が含有しない → `RST_STREAM(ErrorCode::InternalError)` と `Error::InvalidArgument`
  - 統合: selected_protocol = `None` + WT-Available-Protocols 不在 → 正常受理 (既存テストの後方互換)
  - 統合: WT-Available-Protocols 不在 + `selected_protocol = Some(_)` → `RST_STREAM(ErrorCode::InternalError)` と `Error::InvalidArgument`
- 統合テストの `Error::InvalidArgument` 期待は、0077 マージ後に `tokio_http2::Error::WebTransport(_)` への書き換えが必要になる可能性がある。実装時点で 0077 がマージ済みならそちらに合わせる
- CHANGES.md `## develop` に `[CHANGE]` エントリを追加し、`accept()` シグネチャ拡張と WT-Available-Protocols / WT-Protocol サポートを記載すること (issue 番号は含めない)

## 解決方法

### 1. Sans I/O 層: WtAvailableProtocols と sf-string 関連

`src/webtransport/protocols.rs` (新規):

```rust
use crate::webtransport::error::WtError;

#[derive(Debug, Default, Clone)]
pub struct WtAvailableProtocols {
    /// 仕様順 (= 入力順) を保持する。RFC 8941 List のセマンティクス。
    pub protocols: Vec<String>,
}

impl WtAvailableProtocols {
    pub fn parse(value: &[u8]) -> Result<Self, WtError> {
        // RFC 8941 Section 4.2.1 (Parsing a List) と Section 4.2.5 (Parsing a String)
        // に基づく必要最小限実装。要素が sf-string でない場合は WtError::invalid_input。
        // パラメータは読み飛ばす (`;param=value` は値の後ろで discard)。
        ...
    }
}

pub fn serialize_wt_protocol(value: &[u8]) -> Result<Vec<u8>, WtError> {
    // RFC 8941 Section 4.1.6 (Serializing a String) L924:
    // sf-string = DQUOTE *( SP / VCHAR with \ and " escaped ) DQUOTE
    // ASCII printable (0x20-0x7E) 以外の値は invalid_input。
    for &b in value {
        if !(0x20..=0x7E).contains(&b) {
            return Err(WtError::invalid_input(
                "wt-protocol value contains non-printable byte",
            ));
        }
    }
    let mut out = Vec::new();
    out.push(b'"');
    for &b in value {
        if b == b'"' || b == b'\\' {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b'"');
    Ok(out)
}
```

`src/webtransport/mod.rs` に以下を追加:

```rust
pub mod protocols;
pub use protocols::{WtAvailableProtocols, serialize_wt_protocol};
```

`tests/test_webtransport/main.rs` に以下を追加:

```rust
mod protocols;
```

パーサーの要点:

- 入力バイト範囲: ASCII (0x00-0x7F) 外を含む入力は `WtError::invalid_input` (RFC 8941 §4.2 step 1 「Convert input_bytes into an ASCII string input_string; if conversion fails, fail parsing」)
- 空入力 `b""` は `Ok(WtAvailableProtocols { protocols: vec![] })` を返す (RFC 8941 §4.2.1 step 3 「No structured data has been found; return members (which is empty)」準拠)
- 要素間の `,` 区切りと OWS (SP / HTAB) 処理
- sf-string パース: `"` で開始 → `"` までを読む。`\` の直後は `"` または `\` のみ許可、それ以外は失敗。エスケープは解除して内容を保持する。値域は 0x20-0x7E のみ
- bare item / inner-list の先頭文字 (`-`, DIGIT, `?`, `:`, `*`, ALPHA, `(`) が来たら値型不一致として `WtError::invalid_input` (= field 全体を無視)。init.rs のように Token / Boolean / Integer / Inner List を読み飛ばす必要はなく、「`"` 以外で始まる bare item は即 fail」のシンプル実装で済む
- 値の後ろのパラメータ (`;` 始まり) は読み飛ばす
- trailing comma は許容しない (RFC 8941 §4.2.1)

ASCII printable 範囲チェック (0x20-0x7E) は init.rs と同じ定数 `(0x20..=0x7E)` を使う。共通化は両モジュールが安定してから別 issue で扱う。

### 2. tokio-http2 層: helper と accept() への組み込み

`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest` に helper を追加:

```rust
impl WtServerRequest {
    /// `WT-Available-Protocols` ヘッダー値を取得する
    #[must_use]
    pub fn wt_available_protocols(&self) -> Option<&[u8]> {
        self.header(b"wt-available-protocols")
    }
}
```

`accept()` のシグネチャを以下に変更する。`use shiguredo_http2::webtransport::{WtAvailableProtocols, serialize_wt_protocol};` を `use` 文に追加:

```rust
pub async fn accept(
    mut self,
    mut config: WtConfig,
    allowed_origin: Option<&[u8]>,
    selected_protocol: Option<&[u8]>,
) -> Result<WtServerSession> {
    // 1. TLS バージョンチェック (0063 で追加)
    // ... ServerConnection::with_tls(...) ...

    // 2. 部分ムーブ前に &self 借用が必要な値を取得する
    let origin = self.origin().map(|o| o.to_vec());
    let init_bytes = self.webtransport_init().map(|v| v.to_vec());
    let available_bytes = self.wt_available_protocols().map(|v| v.to_vec());
    let Self { mut conn, stream_id, .. } = self;

    // 3. Origin 検証 (0062 で実装済み、403 自動送信)
    // ...

    // 4. WebTransport-Init パースとマージ (0064 で追加)
    // ...

    // 5. WT-Available-Protocols パースと selected_protocol 検証 (本 issue)
    // draft-ietf-webtrans-http3-15 Section 3.3 (L538-L580):
    // WT-Available-Protocols は RFC 8941 List of String。
    // パース失敗は仕様上 ignore (= ヘッダー不在扱い)。
    let available = available_bytes
        .as_deref()
        .and_then(|bytes| WtAvailableProtocols::parse(bytes).ok());

    if let Some(protocol_bytes) = selected_protocol {
        // available が None (= ヘッダー不在 or パース失敗 ignore) または空リストの場合も listed = false
        let listed = available
            .as_ref()
            .map(|av| av.protocols.iter().any(|p| p.as_bytes() == protocol_bytes))
            .unwrap_or(false);
        if !listed {
            conn.reset_stream(stream_id, ErrorCode::InternalError).await?;
            return Err(Error::InvalidArgument(
                "selected_protocol is not listed in WT-Available-Protocols".into(),
            ));
        }
    }

    // 6. :status=200 送信 (selected_protocol があれば WT-Protocol を追加)
    let mut response = vec![HeaderField::from_static(b":status", b"200")];
    if let Some(protocol_bytes) = selected_protocol {
        let serialized = match serialize_wt_protocol(protocol_bytes) {
            Ok(s) => s,
            Err(e) => {
                conn.reset_stream(stream_id, ErrorCode::InternalError).await?;
                return Err(Error::InvalidArgument(format!("serialize wt-protocol: {e}")));
            }
        };
        response.push(
            HeaderField::new(b"wt-protocol", &serialized)
                .expect("serialized wt-protocol is a valid header value"),
        );
    }
    conn.send_response(stream_id, response, false).await?;

    // 7. WtSession 生成、selected_protocol を WtServerSession に保存
    let selected_protocol_owned = selected_protocol.map(|p| p.to_vec());
    // ... WtServerSession { session_id, cmd_tx, ..., selected_protocol: selected_protocol_owned } ...
}
```

`WtServerSession` 構造体 (`webtransport.rs` L274-L281) に `selected_protocol: Option<Vec<u8>>` フィールドを追加し、公開アクセサを追加:

```rust
impl WtServerSession {
    #[must_use]
    pub fn selected_protocol(&self) -> Option<&[u8]> {
        self.selected_protocol.as_deref()
    }
}
```

`WtSessionParts` (L381-L395) にも同フィールドを追加し、`into_parts()` で値を移譲する。

### 3. 既存呼び出し側の更新

`accept()` シグネチャ変更に伴い以下を更新する:

- `crates/tokio-http2/tests/test_webtransport.rs` (tokio-http2 クレートの統合テスト、単一ファイル): 既存 `test_wt_*` 全件で `accept(config, None)` を `accept(config, None, None)` に書き換える
- `examples/wt_server`: 同様に `selected_protocol` 引数 `None` を追加

注意: Sans I/O 層の単体テストはプロジェクトルートの `tests/test_webtransport/` ディレクトリ配下に置く。tokio-http2 層の統合テストは `crates/tokio-http2/tests/test_webtransport.rs` に置く。両者は別の場所である。

新規テストは以下を `test_webtransport.rs` に追加 (既存の `connect_request_with_origin` パターンを参考に `connect_request_with_protocols(protocols: &str)` helper を作る):

- 正常系: `wt-available-protocols: "echo"` + `selected_protocol = Some(b"echo")` → 成功し、レスポンスに `wt-protocol: "echo"` が含まれる
- 含有しない: `wt-available-protocols: "echo"` + `selected_protocol = Some(b"raw")` → `RST_STREAM(ErrorCode::InternalError)` と `Error::InvalidArgument`
- 不在 + 指定: WT-Available-Protocols なし + `selected_protocol = Some(b"echo")` → `RST_STREAM(ErrorCode::InternalError)` と `Error::InvalidArgument`
- selected_protocol = None: 既存テストの後方互換確認

統合テストで WT-Protocol レスポンスヘッダーを検証するには、`perform_connect` 等の helper を返り値付きに変更するか、新たにレスポンスヘッダーを取得する helper を追加する。

注意: 0077 マージ後は `tokio_http2::Error` に `WebTransport(WtError)` バリアントが追加されるため、本 issue で追加する「selected_protocol 不在・含有しない・シリアライズ失敗」ケースの統合テスト期待値を `Error::InvalidArgument` から `Error::WebTransport(_)` に書き換える必要がある。実装時点で 0077 がマージ済みならそちらに合わせる。

### 4. テスト戦略

Sans I/O 単体テスト (`tests/test_webtransport/protocols.rs`、AGENTS.md 規約に従いテストログは日本語):

- 完了条件で列挙したすべての境界ケース
- `WtAvailableProtocols::parse` の正常系・空・型不一致・パラメータ・エスケープ解除・重複
- `serialize_wt_protocol` の正常系・エスケープ・非 printable

統合テスト (`crates/tokio-http2/tests/test_webtransport.rs`):

- 完了条件 (上記) の 4 ケース

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 3.3 (Application Protocol Negotiation), L317-L342 — HTTP/2 側の概要 (本文 L341-L342 で「Section 3.4 of [WEBTRANSPORT-H3]」と参照誤りしているが、現行 draft-ietf-webtrans-http3-15 では Section 3.3 が定義箇所)
- draft-ietf-webtrans-http3-15 Section 3.3 (Application Protocol Negotiation), L531-L585 — WT-Available-Protocols / WT-Protocol の実定義
  - L543: 「the server MUST include a single choice from the client's list」
  - L550-L551: 「Any value type other than String MUST be treated as an error that causes the entire field to be ignored」
  - L553-L554: 「parameters MUST be ignored」
  - L565-L567: 「Servers MAY reject ... if the WT-Available-Protocols header field is absent or if it is malformed and therefore ignored」
  - L569-L572: クライアント側 WT_ALPN_ERROR (WT-Protocol が含まれない場合)
  - L574-L580: クライアント側 WT_ALPN_ERROR (受信値が available にない場合)
- RFC 8941 Section 3.1 (Lists), L322 — 「Lists are arrays of zero or more members」、ABNF (`sf-list`) は L330
- RFC 8941 Section 3.3 (Items), L508
- RFC 8941 Section 3.3.3 (Strings), L581-L613 — sf-string 値域 0x20-0x7E、`"` と `\` のエスケープ、ABNF は L589-L591
- RFC 8941 Section 4.1.6 (Serializing a String), L924 — 値域外バイト拒否は L932-L933
- RFC 8941 Section 4.2.1 (Parsing a List), L1086 — 空入力時は step 3 (L1112-L1113) で empty members を返す
- RFC 8941 Section 4.2.3 (Parsing an Item), L1215
- RFC 8941 Section 4.2.5 (Parsing a String), L1382
- RFC 9110 Section 15.5.7 (406 Not Acceptable) — 呼び出し側が `reject(406)` を選ぶ場合の参照
- RFC 9113 Section 7 (Error Codes) — `RST_STREAM` の `InternalError` (0x02) の用途

## 依存関係

- 前提: 0062 (Origin 検証、closed)、0063 (TLS バージョン要件チェック、closed)、0064 (WebTransport-Init、closed) — `accept()` 内処理順序は 0063 設計判断 5 で「TLS → Origin → 0064 → `:status=200` 送信」という大枠が確定しており、本 issue はその直前にサブプロトコル検証を挿入する。0063 設計判断 5 は当初「WT-Protocol を 200 送信の後ろに置く」想定だったが、本 issue 設計判断 4 (失敗時に 200 送信前に RST_STREAM) の要件により 200 送信の前に組み込む
- 関連: 0065 (TLS Keying Material Exporter) — `accept()` シグネチャ変更の影響を受けないが、両方が open であるため `crates/tokio-http2/src/webtransport.rs` および統合テストで変更箇所が競合する可能性がある。実装時は最新の develop を取り込む
- 関連: 0068 (`tests/test_webtransport/main.rs` に `mod error;` 追加) — 0068 と本 issue は同じ `tests/test_webtransport/main.rs` を編集するため、マージ順序によりコンフリクト解決が必要。最終順序は `mod capsule;` → `mod error;` (0068) → `mod exporter;` (0065) → `mod flow_control;` → `mod init;` → `mod integration;` → `mod protocols;` (本 issue) → 残り
- 関連: 0074 (WebTransport over HTTP/2 参照 draft の更新) — 0074 マージ後、issue 本文の `draft-ietf-webtrans-http2-14 Section 3.3 L317-L342` の行番号は古くなる。WEBTRANSPORT-H3 -15 側 (L531-L585) は別途 update-refs 対象。可能であれば 0074 を先にマージしてから本 issue を実装する
- 関連: 0077 (tokio-http2 のエラー型整理) — 0077 マージ後は `Error::InvalidArgument` への文字列化経路が `Error::WebTransport(WtError)` に統合される。本 issue の `accept()` 内で追加する `.map_err(wt_err)` は 0077 実装時の置換対象となり、本 issue マージ後は `wt_err` 呼び出しが 1 箇所増える。0077 が 0066 より先にマージされた場合、実装時点で既存パターンに合わせてエラー変換を行うこと
- 実装順序の推奨: 0063 → 0064 → 0065/0066 (並行可) → 0074 後に 0066 の行番号引用を最終確認 → 0077
