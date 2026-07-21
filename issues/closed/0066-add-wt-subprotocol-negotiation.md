# WT-Available-Protocols / WT-Protocol サブプロトコルネゴシエーションを追加する

- Priority: Medium
- Created: 2026-06-08
- Completed: 2026-07-21
- Polished: 2026-06-09
- Model: deepseek-v4-pro
- Branch: feature/change-wt-draft15-remaining

## 目的

draft-ietf-webtrans-http2-14 Section 3.3 に定義されているサブプロトコルネゴシエーション機能 (WT-Available-Protocols リクエストヘッダー / WT-Protocol レスポンスヘッダー) をサーバー側に実装し、ALPN ライクなサブプロトコル選択を可能にする。

## 優先度根拠

仕様上の MAY 要件 (サブプロトコルは必須ではない)。既存プロトコルの WebTransport 移植に有用だが必須ではないため Priority は Medium。

## 現状

draft-ietf-webtrans-http2-14 Section 3.3 (L317-L342):

> The user agent MAY include a WT-Available-Protocols header field in the CONNECT request.  The WT-Available-Protocols enumerates the possible protocols in preference order.  If the server receives such a header, it MAY include a WT-Protocol field in a successful (2xx) response.  If it does, the server MUST include a single choice from the client's list in that field.  Servers MAY reject the request if the client did not include a suitable protocol.
>
> Both WT-Available-Protocols and WT-Protocol are defined in Section 3.4 of [WEBTRANSPORT-H3].

注: HTTP/2 ドラフト L341-L342 は「Section 3.4 of [WEBTRANSPORT-H3]」と参照するが、これは仕様本体の参照誤り。現行 draft-ietf-webtrans-http3-14 では **Section 3.3** が両ヘッダーの定義箇所 (Section 3.4 は "Prioritization")。実装時は WEBTRANSPORT-H3 Section 3.3 を参照する。

draft-ietf-webtrans-http3-14 Section 3.3 (`refs/draft-ietf-webtrans-http3-14.txt` L1499-L1525) の主要要件:

- `WT-Available-Protocols` は **RFC 8941 List of String** (preference order、最優先が先頭)
- `WT-Protocol` は **RFC 8941 Item of String**
- 値型は String のみ。String 以外の値型は MUST「field 全体を無視」(L1516-L1519)
- パラメータには意味は定義されない。MUST「パラメータは無視」(L1518-L1519)
- WT-Protocol の値は WT-Available-Protocols のいずれかに含まれていなければならない (L1520-L1522)。受信側は含まれない場合 WT-Protocol を MUST 無視

現在の実装: `grep -rn "wt-available-protocols\|wt-protocol\|selected_protocol" src/ crates/` でヒットなし、完全な未実装。

## 本 issue で扱う

- Sans I/O 層: `WtAvailableProtocols` 構造体 (preference order を保持する `Vec<String>` ラッパ) と `WtAvailableProtocols::parse(value: &[u8]) -> Result<WtAvailableProtocols, WtError>` を `src/webtransport/protocols.rs` (新規) に追加。`serialize_wt_protocol(value: &[u8]) -> Result<Vec<u8>, WtError>` (sf-string シリアライズ) も同モジュールに追加
- tokio-http2 層:
  - `WtServerRequest::wt_available_protocols(&self) -> Option<&[u8]>` (生バイト列 helper) を追加
  - `WtServerRequest::accept()` のシグネチャに `selected_protocol: Option<&[u8]>` 引数を追加
  - `accept()` 内で 0063 設計判断 5 に従い「TLS → Origin → 0064 Init → 0066 サブプロトコル検証 → :status=200 (WT-Protocol 含む)」の順に処理
  - `WtServerSession` / `WtSessionParts` に `selected_protocol: Option<Vec<u8>>` フィールドを追加し、`WtServerSession::selected_protocol(&self) -> Option<&[u8]>` を公開

## 本 issue のスコープ外

- **クライアント側の WT-Available-Protocols 送信 / WT-Protocol 受信検証**: 現状 `crates/tokio-http2/src/client.rs` に WebTransport クライアント API が無いため対象外。クライアント側 WebTransport API 追加時に併せて対応する
- **`sfv` クレートなど外部依存の追加**: 0064 と同じく依存最小化方針 (CLAUDE.md / shiguredo-rust 規約) に従い必要最小限の自前パーサーで対応する
- **suitable protocol が無い場合の `MAY reject`**: 仕様 (L1512-L1513) は MAY なので、本実装ではサーバー側の自動拒否は行わない。呼び出し側が `wt_available_protocols()` を見て不適合と判断した場合、明示的に `reject(406)` を呼ぶ (`reject` は既存 API)
- **複数 `wt-available-protocols` ヘッダーの結合**: 0064 と同様、最初の 1 個のみを評価する。複数行結合は将来必要になれば別 issue で対応する

## 設計判断

### 1. RFC 8941 List of String / Item of String パーサーは Sans I/O 層に新規モジュール (`src/webtransport/protocols.rs`) を作る

0064 は Dictionary + Integer の最小パーサーを `src/webtransport/init.rs` に置くが、本 issue は List + Item + String の別ロジックを必要とする (Dictionary key 抽出と List 要素抽出は異なる)。責務を分けるため別モジュールに置く。共通化は両方が安定してから別 issue で検討する。

### 2. パース失敗時の挙動: 「field を無視」(仕様準拠)

仕様 L1516-L1519 は「String 以外の値型 → MUST 全体を無視」と規定する。これは「parse error で 4xx を返す」(0064 の WebTransport-Init) とは挙動が異なる。

`WtAvailableProtocols::parse` は仕様違反 (String 以外の値、パース不能) の場合 `WtError::invalid_input` を返す。`accept()` 内では `Result::ok()` で `Option` に畳み込み、`None` の場合は「ヘッダー不在」と等価に扱う (= サブプロトコルなし)。**4xx は返さない**。

### 3. WT-Protocol 値の型は `Option<&[u8]>` で API 統一

`accept()` の引数は既存の `allowed_origin: Option<&[u8]>` と統一するため `selected_protocol: Option<&[u8]>` を採用する。RFC 8941 sf-string は ASCII printable (0x20-0x7E) のみだが、`accept()` 内でバリデーション (0x20-0x7E のみ、DQUOTE と `\` のエスケープ可能性は serializer 側で処理) を行うことで型安全性を担保する。バリデーション失敗時は `Error::InvalidArgument` (呼び出し側のミス扱い)。

戻り値型は `WtServerSession::selected_protocol(&self) -> Option<&[u8]>` で統一。

### 4. selected_protocol のクライアントリスト含有検証

仕様 L1520-L1522 「The value in the WT-Protocol response header field MUST be one of the values listed in WT-Available-Protocols of the request.」に従い、`selected_protocol` が `Some(_)` の場合は WT-Available-Protocols をパースして含有検証を行う。以下のいずれも呼び出し側のミスとして `Error::InvalidArgument` を返し、`accept()` は失敗させる (レスポンス送信なし、CONNECT ストリームに何も書かない):

- クライアントリスト不在 (WT-Available-Protocols ヘッダーがない、または設計判断 2 によりパース失敗で ignore されている)
- クライアントリストに `selected_protocol` が含まれていない

仕様 L1510-L1511 「If the server receives such a header, it MAY include a WT-Protocol field」より、WT-Available-Protocols を受け取っていない場合に WT-Protocol を返すのは仕様上未定義動作のため、不在ケースもエラー扱いとする。

### 5. accept() シグネチャ拡張は破壊的変更で CHANGES.md は `[CHANGE]`

`accept(self, config, allowed_origin)` → `accept(self, config, allowed_origin, selected_protocol)` の引数追加。既存呼び出し側 (`crates/tokio-http2/tests/test_webtransport.rs` など) は全件更新が必要。CHANGES.md `## develop` セクションに `[CHANGE]` エントリを追加する (0062 の Origin 検証拡張 (L11) と同じ扱い)。

### 6. WtServerSession / WtSessionParts への selected_protocol フィールド追加

`WtServerSession::selected_protocol()` が `&[u8]` を返すには構造体にフィールドが必要。`accept()` が確定値を `Option<Vec<u8>>` で持ち、`into_parts()` で `WtSessionParts` にも同フィールドを伝播する。driver タスクは selected_protocol を扱わない (TLS exporter のようなランタイム参照は不要)。

### 7. レスポンスの WT-Protocol シリアライズ

sf-string は `DQUOTE *( SP / VCHAR with \ and " escaped ) DQUOTE` (RFC 8941 §4.1.6)。`serialize_wt_protocol(value: &[u8])` は値を DQUOTE で囲み、`"` と `\` を `\` でエスケープする。これにより `selected_protocol = b"echo"` は wire 上で `"echo"` (DQUOTE 込み 6 バイト) となる。

`HeaderField::new(b"wt-protocol", serialized.as_slice())?` でレスポンスヘッダーリストに追加する。`HeaderField::from_static` は使えない (実行時値のため)。

### 8. チェック順序

0063 設計判断 5 で確定した順序に従う:

1. TLS バージョンチェック (0063)
2. Origin 検証 (0062)
3. WebTransport-Init パース・マージ (0064)
4. **WT-Available-Protocols パース + selected_protocol 検証 (本 issue)**
5. `:status=200` 送信 (WT-Protocol を含める) と `WtSession` 生成

部分ムーブ前に `&self` 借用が必要な値 (origin / init_bytes / available_bytes) はすべて取得しておき、`let Self { mut conn, stream_id, .. } = self;` で部分ムーブする。

## 完了条件

- `src/webtransport/protocols.rs` (新規) に以下が追加されていること:
  - `pub struct WtAvailableProtocols { pub protocols: Vec<String> }`
  - `WtAvailableProtocols::parse(value: &[u8]) -> Result<WtAvailableProtocols, WtError>`
  - `pub fn serialize_wt_protocol(value: &[u8]) -> Result<Vec<u8>, WtError>` (sf-string シリアライズ)
- `src/webtransport/mod.rs` に `pub mod protocols;` と `pub use protocols::{WtAvailableProtocols, serialize_wt_protocol};` が追加されていること
- `WtAvailableProtocols::parse` が以下を満たすこと:
  - 正常系: `"echo", "raw"` で `protocols = ["echo", "raw"]` (preference order = 入力順を保持)
  - 単一エントリ: `"echo"` で `protocols = ["echo"]`
  - 空 List (= 空入力): `protocols = []`
  - String 以外の値型 (`echo` (Token), `123` (Integer), `?1` (Boolean), `:YWJj:` (Byte Sequence)): `WtError::invalid_input` を返す
  - パラメータ無視: `"echo";version=1, "raw";v=2` で `protocols = ["echo", "raw"]`
  - DQUOTE エスケープ: `"hello\"world"` で `protocols = ["hello\"world"]`
  - 重複 (sf-list は重複可): `"a", "a", "b"` で `protocols = ["a", "a", "b"]`
- `serialize_wt_protocol` が以下を満たすこと:
  - 正常系: `b"echo"` → `b"\"echo\""` (DQUOTE 込み)
  - エスケープ: `b"a\"b"` → `b"\"a\\\"b\""` (`"` を `\"` に、`\` を `\\` に)
  - ASCII printable 外 (0x20 未満、0x7F 以上): `WtError::invalid_input`
- `WtServerRequest::wt_available_protocols(&self) -> Option<&[u8]>` が追加され、`b"wt-available-protocols"` (HTTP/2 lowercase) と一致するヘッダーの値を返すこと
- `WtServerRequest::accept(self, mut config: WtConfig, allowed_origin: Option<&[u8]>, selected_protocol: Option<&[u8]>) -> Result<WtServerSession>` シグネチャに変更されていること
- `accept()` が以下を満たすこと:
  - `selected_protocol == None` の場合: WT-Available-Protocols の有無に関わらず正常受理。WT-Protocol レスポンスヘッダーは付与しない
  - `selected_protocol == Some(p)` かつ WT-Available-Protocols がパース可能で `p` が含まれる場合: 正常受理。`:status=200` レスポンスに `wt-protocol: "p"` (sf-string serialized) を追加
  - `selected_protocol == Some(p)` かつ WT-Available-Protocols 不在 / パース失敗 / 含有しない場合: `Error::InvalidArgument` を返す (レスポンス送信なし)
  - `selected_protocol == Some(p)` で `p` が ASCII printable 外を含む場合: `Error::InvalidArgument` を返す
- `WtServerSession` 構造体に `selected_protocol: Option<Vec<u8>>` フィールドが追加され、`pub fn selected_protocol(&self) -> Option<&[u8]>` が公開されていること
- `WtSessionParts` 構造体にも `selected_protocol: Option<Vec<u8>>` フィールドが追加され、`into_parts()` で値が伝播すること
- 既存の `WtServerRequest::accept()` 呼び出し側 (`crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_*` 12 件以上、`examples/wt_server` 1 件) がすべて `selected_protocol` 引数 (`None` または明示値) を追加して更新されていること
- 単体テスト・統合テストが以下を検証していること:
  - Sans I/O 単体: 上記 `WtAvailableProtocols::parse` / `serialize_wt_protocol` の各境界ケース
  - 統合: WT-Available-Protocols が `"echo"` で `selected_protocol = Some(b"echo")` → 成功し、クライアントが WT-Protocol = `"echo"` を受信
  - 統合: selected_protocol が含有しない → `Error::InvalidArgument`
  - 統合: selected_protocol = `None` + WT-Available-Protocols 不在 → 正常受理 (既存テストの後方互換)
- CHANGES.md `## develop` に `[CHANGE]` エントリを追加し、`accept()` シグネチャ拡張と WT-Available-Protocols / WT-Protocol サポートを記載すること

## 解決方法

- `src/webtransport/protocols.rs` を新規追加し、`WtAvailableProtocols::parse` (RFC 8941 List of String) と `serialize_wt_protocol` (sf-string) を実装した。パース失敗は field 無視用の `invalid_input`。
- `WtServerRequest::accept` に `selected_protocol: Option<&[u8]>` を追加し、クライアント一覧含有検証後に `wt-protocol` レスポンスヘッダーを付与する。`wt_available_protocols()` helper と `selected_protocol()` アクセサも追加した。
- Sans I/O 単体テスト (`tests/test_webtransport/protocols.rs`) と tokio-http2 統合テスト (選択成功 / リスト外拒否) を追加した。
- draft-15 残り対応と同じブランチ `feature/change-wt-draft15-remaining` で実装した。

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 3.3 (Application Protocol Negotiation), L317-L342
- draft-ietf-webtrans-http3-14 Section 3.3 (Application Protocol Negotiation), L1499-L1525 — WT-Available-Protocols / WT-Protocol の実定義 (HTTP/2 ドラフトは「Section 3.4」と参照誤りしているが現行 -14 では Section 3.3)
- RFC 8941 Section 3.1 (Lists)
- RFC 8941 Section 3.3 (Items)
- RFC 8941 Section 3.3.3 (Strings) — sf-string 値域 0x20-0x7E、`"` と `\` のエスケープ
- RFC 8941 Section 4.1.6 (Serializing a String)
- RFC 8941 Section 4.2.1 (Parsing a List)
- RFC 8941 Section 4.2.3 (Parsing an Item)
- RFC 8941 Section 4.2.5 (Parsing a String)
- RFC 9110 Section 15.5.7 (406 Not Acceptable) — 呼び出し側が `reject(406)` を選ぶ場合の参照

## 依存関係

- 前提: 0062 (Origin 検証、closed)、0063 (TLS バージョン要件チェック)、0064 (WebTransport-Init) — `accept()` 内処理順序は 0063 設計判断 5 で「TLS → Origin → 0064 → 0066 → :status=200」と確定済み
- 実装順序: 0063 → 0064 → 0066
