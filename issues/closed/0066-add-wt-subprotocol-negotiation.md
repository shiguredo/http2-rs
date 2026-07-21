# WT-Available-Protocols / WT-Protocol サブプロトコルネゴシエーションを追加する

- Priority: Medium
- Created: 2026-06-08
- Completed: 2026-07-21
- Polished: 2026-07-21
- Model: deepseek-v4-pro
- Branch: feature/change-wt-draft15-remaining

## 目的

draft-ietf-webtrans-http2-15 Section 3.3 (L366-L383) に定義されているサブプロトコルネゴシエーション機能 (WT-Available-Protocols リクエストヘッダー / WT-Protocol レスポンスヘッダー) をサーバー側に実装し、ALPN ライクなサブプロトコル選択を可能にする。

## 優先度根拠

仕様上の MAY 要件 (サブプロトコルは必須ではない)。既存プロトコルの WebTransport 移植に有用だが必須ではないため Priority は Medium。

## 現状

draft-ietf-webtrans-http2-15 Section 3.3 (L366-L383):

> WebTransport over HTTP/2 offers a subprotocol negotiation mechanism, similar to TLS Application-Layer Protocol Negotiation Extension (ALPN) [RFC7301]; the intent is to simplify porting pre-existing protocols that rely on this type of functionality.
>
> The client MAY include a WT-Available-Protocols header field in the CONNECT request. The WT-Available-Protocols enumerates the possible protocols in preference order. If the server receives such a header, it MAY include a WT-Protocol field in a successful (2xx) response. If it does, the server MUST include a single choice from the client's list in that field. Servers MAY reject the request if the client did not include a suitable protocol.
>
> Both WT-Available-Protocols and WT-Protocol are defined in Section 3.3 of [WEBTRANSPORT-H3].

draft-ietf-webtrans-http3-15 Section 3.3 (`refs/draft-ietf-webtrans-http3-15.txt` L531-L581) の主要要件:

- `WT-Available-Protocols` と `WT-Protocol` は Structured Fields [FIELDS]。`WT-Available-Protocols` は **List**、`WT-Protocol` は **Item**。いずれも有効な値型は **String** のみ
- String 以外の値型は MUST「field 全体を無視」(L550-L552)
- パラメータには意味は定義されない。MUST「パラメータは無視」(L552-L554)
- サーバーが WT-Available-Protocols を受け取った場合、WT-Protocol を返すのは MAY。返す場合はクライアントリストから 1 つを選ぶ MUST (L541-L544)
- WT-Protocol の値は WT-Available-Protocols のいずれかに含まれていなければならない MUST (L573-L576)。クライアントは含まれない場合 **WT_ALPN_ERROR でセッションをクローズ** MUST (L576-L579)
- サーバーは WT-Available-Protocols が不在または malformed な場合にセッションを拒否 MAY (L564-L566)

現在の実装: `grep -rn "wt-available-protocols\|wt-protocol\|selected_protocol" src/ crates/` でヒットなし、完全な未実装。

## 本 issue で扱う

- Sans I/O 層: `WtAvailableProtocols` 構造体 (preference order を保持する `Vec<String>` ラッパ) と `WtAvailableProtocols::parse(value: &[u8]) -> WtResult<WtAvailableProtocols>` を `src/webtransport/protocols.rs` (新規) に追加。`serialize_wt_protocol(value: &[u8]) -> WtResult<Vec<u8>>` (sf-string シリアライズ) も同モジュールに追加
- tokio-http2 層:
  - `WtServerRequest::wt_available_protocols(&self) -> Option<&[u8]>` (生バイト列 helper) を追加
  - `WtServerRequest::accept()` のシグネチャに `selected_protocol: Option<&[u8]>` 引数を追加
  - `accept()` 内で「TLS → Origin → WebTransport-Init → サブプロトコル検証 → :status=200 (WT-Protocol 含む)」の順に処理
  - `WtServerSession` / `WtSessionParts` に `selected_protocol: Option<Vec<u8>>` フィールドを追加し、`WtServerSession::selected_protocol(&self) -> Option<&[u8]>` を公開

## 本 issue のスコープ外

- **クライアント側の WT-Available-Protocols 送信 / WT-Protocol 受信検証**: 現状 `crates/tokio-http2/src/client.rs` に WebTransport クライアント API が無いため対象外。クライアント側 WebTransport API 追加時に併せて対応する
- **`sfv` クレートなど外部依存の追加**: 0064 と同じく依存最小化方針 (AGENTS.md / shiguredo-rust 規約) に従い必要最小限の自前パーサーで対応する
- **suitable protocol が無い場合の `MAY reject`**: 仕様は MAY なので、本実装ではサーバー側の自動拒否は行わない。呼び出し側が `wt_available_protocols()` を見て不適合と判断した場合、明示的に `reject(406)` を呼ぶ (`reject` は既存 API)
- **複数 `wt-available-protocols` ヘッダーの結合**: 0064 と同様、最初の 1 個のみを評価する。複数行結合は将来必要になれば別 issue で対応する

## 設計方針

### 1. RFC 8941 List / Item パーサーは Sans I/O 層に新規モジュール (`src/webtransport/protocols.rs`) を作る

0064 は Dictionary + Integer の最小パーサーを `src/webtransport/init.rs` に置くが、本 issue は List + Item + String の別ロジックを必要とする (Dictionary key 抽出と List 要素抽出は異なる)。責務を分けるため別モジュールに置く。共通化は両方が安定してから別 issue で検討する。

### 2. パース失敗時の挙動: 「field を無視」(仕様準拠)

draft-ietf-webtrans-http3-15 Section 3.3 (L550-L552) は「String 以外の値型 → MUST 全体を無視」と規定する。これは「parse error で 4xx を返す」(0064 の WebTransport-Init) とは挙動が異なる。

`WtAvailableProtocols::parse` は仕様違反 (String 以外の値、パース不能) の場合 `WtError::invalid_input` を返す。`accept()` 内では `Result::ok()` で `Option` に畳み込み、`None` の場合は「ヘッダー不在」と等価に扱う (= サブプロトコルなし)。**4xx は返さない**。

### 3. WT-Protocol 値の型は `Option<&[u8]>` で API 統一

`accept()` の引数は既存の `allowed_origin: Option<&[u8]>` と統一するため `selected_protocol: Option<&[u8]>` を採用する。RFC 8941 sf-string は ASCII printable (0x20-0x7E) のみだが、`accept()` 内でバリデーションを行うことで型安全性を担保する。バリデーション失敗時は `Error::InvalidArgument` (呼び出し側のミス扱い)。

`serialize_wt_protocol` 内でも ASCII printable 検証を行うが、これは Sans I/O 層の純粋関数としての自己防衛であり、`accept()` 内の検証と意図的に二重にしている (呼び出し経路が異なるため)。

戻り値型は `WtServerSession::selected_protocol(&self) -> Option<&[u8]>` で統一。

### 4. selected_protocol のクライアントリスト含有検証

draft-ietf-webtrans-http3-15 Section 3.3 (L573-L576) 「the value in the WT-Protocol response header field MUST be one of the values listed in WT-Available-Protocols of the request.」に従い、`selected_protocol` が `Some(_)` の場合は WT-Available-Protocols をパースして含有検証を行う。以下のいずれも呼び出し側のミスとして `Error::InvalidArgument` を返し、`accept()` は失敗させる (レスポンス送信なし、CONNECT ストリームに何も書かない):

- クライアントリスト不在 (WT-Available-Protocols ヘッダーがない、または設計判断 2 によりパース失敗で ignore されている)
- クライアントリストに `selected_protocol` が含まれていない

仕様 (L541-L542) 「If the server receives such a header, it MAY include a WT-Protocol field」より、WT-Available-Protocols を受け取っていない場合に WT-Protocol を返すのは仕様上未定義動作のため、不在ケースもエラー扱いとする。

### 5. accept() シグネチャ拡張は破壊的変更で CHANGES.md は `[CHANGE]`

`accept(mut self, config, allowed_origin)` → `accept(mut self, config, allowed_origin, selected_protocol)` の引数追加。既存呼び出し側 (`crates/tokio-http2/tests/test_webtransport.rs` の 16 件、`examples/wt_server` の 1 件) は全件更新が必要。CHANGES.md `## develop` セクションに `[CHANGE]` エントリを追加する (0062 の Origin 検証拡張と同じ扱い)。

### 6. WtServerSession / WtSessionParts への selected_protocol フィールド追加

`WtServerSession::selected_protocol()` が `&[u8]` を返すには構造体にフィールドが必要。`accept()` が確定値を `Option<Vec<u8>>` で持ち、`into_parts()` で `WtSessionParts` にも同フィールドを伝播する。driver タスクは selected_protocol を扱わない (TLS exporter のようなランタイム参照は不要)。

### 7. レスポンスの WT-Protocol シリアライズ

sf-string は `DQUOTE *( SP / VCHAR with \ and " escaped ) DQUOTE` (RFC 8941 Section 4.1.6)。`serialize_wt_protocol(value: &[u8])` は値を DQUOTE で囲み、`"` と `\` を `\` でエスケープする。これにより `selected_protocol = b"echo"` は wire 上で `"echo"` (DQUOTE 込み 6 バイト) となる。

`HeaderField::new(b"wt-protocol", serialized.as_slice())?` でレスポンスヘッダーリストに追加する。`HeaderField::from_static` は使えない (実行時値のため)。

### 8. チェック順序

0063 設計判断 5 で確定した順序に従う:

1. TLS バージョンチェック (0063)
2. Origin 検証 (0062)
3. WebTransport-Init パース・マージ (0064)
4. **WT-Available-Protocols パース + selected_protocol 検証 (本 issue)**
5. `:status=200` 送信 (WT-Protocol を含める) と `WtSession` 生成

部分ムーブ前に `&self` 借用が必要な値 (origin / init_bytes / available_bytes) はすべて取得しておき、`let Self { mut conn, stream_id, .. } = self;` で部分ムーブする。

## 後方互換

`accept()` シグネチャへの引数追加は破壊的変更 (`[CHANGE]`)。新規モジュール・新規メソッド・新規フィールドは加算的変更。`DriverCmd` への変更はない。

## 完了条件

- `src/webtransport/protocols.rs` (新規) に以下が追加されていること:
  - `pub struct WtAvailableProtocols { pub protocols: Vec<String> }`
  - `WtAvailableProtocols::parse(value: &[u8]) -> WtResult<WtAvailableProtocols>`
  - `pub fn serialize_wt_protocol(value: &[u8]) -> WtResult<Vec<u8>>` (sf-string シリアライズ)
- `src/webtransport/mod.rs` に `pub mod protocols;` と `pub use protocols::{WtAvailableProtocols, serialize_wt_protocol};` が追加されていること
- `WtAvailableProtocols::parse` が以下を満たすこと:
  - 正常系: `"echo", "raw"` で `protocols = ["echo", "raw"]` (preference order = 入力順を保持)
  - 単一エントリ: `"echo"` で `protocols = ["echo"]`
  - 空 List (= 空入力): `protocols = []` (RFC 8941 Section 4.2.1 で空入力は空 List として有効)
  - String 以外の値型 (`echo` (Token), `123` (Integer), `?1` (Boolean), `:YWJj:` (Byte Sequence)): `WtError::invalid_input` を返す
  - パラメータ無視: `"echo";version=1, "raw";v=2` で `protocols = ["echo", "raw"]`
  - DQUOTE エスケープ: `"hello\"world"` で `protocols = ["hello\"world"]`
  - 重複 (sf-list は重複可): `"a", "a", "b"` で `protocols = ["a", "a", "b"]`
- `serialize_wt_protocol` が以下を満たすこと:
  - 正常系: `b"echo"` → `b"\"echo\""` (DQUOTE 込み)
  - エスケープ: `b"a\"b"` → `b"\"a\\\"b\""` (`"` を `\"` に、`\` を `\\` に)
  - ASCII printable 外 (0x20 未満、0x7F 以上): `WtError::invalid_input`
- `WtServerRequest::wt_available_protocols(&self) -> Option<&[u8]>` が追加され、`b"wt-available-protocols"` (HTTP/2 lowercase) と一致するヘッダーの値を返すこと
- `WtServerRequest::accept(mut self, mut config: WtConfig, allowed_origin: Option<&[u8]>, selected_protocol: Option<&[u8]>) -> Result<WtServerSession>` シグネチャに変更されていること
- `accept()` が以下を満たすこと:
  - `selected_protocol == None` の場合: WT-Available-Protocols の有無に関わらず正常受理。WT-Protocol レスポンスヘッダーは付与しない
  - `selected_protocol == Some(p)` かつ WT-Available-Protocols がパース可能で `p` が含まれる場合: 正常受理。`:status=200` レスポンスに `wt-protocol: "p"` (sf-string serialized) を追加
  - `selected_protocol == Some(p)` かつ WT-Available-Protocols 不在 / パース失敗 / 含有しない場合: `Error::InvalidArgument` を返す (レスポンス送信なし)
  - `selected_protocol == Some(p)` で `p` が ASCII printable 外を含む場合: `Error::InvalidArgument` を返す
- `WtServerSession` 構造体に `selected_protocol: Option<Vec<u8>>` フィールドが追加され、`pub fn selected_protocol(&self) -> Option<&[u8]>` が公開されていること
- `WtSessionParts` 構造体にも `selected_protocol: Option<Vec<u8>>` フィールドが追加され、`into_parts()` で値が伝播すること
- 既存の `WtServerRequest::accept()` 呼び出し側 (`crates/tokio-http2/tests/test_webtransport.rs` の 16 件、`examples/wt_server` の 1 件) がすべて `selected_protocol` 引数 (`None` または明示値) を追加して更新されていること
- `tests/test_webtransport/main.rs` に `mod protocols;` がアルファベット順で追加されていること
- 単体テスト・統合テストが以下を検証していること:
  - Sans I/O 単体: 上記 `WtAvailableProtocols::parse` / `serialize_wt_protocol` の各境界ケース
  - 統合: WT-Available-Protocols が `"echo"` で `selected_protocol = Some(b"echo")` → 成功し、クライアントが WT-Protocol = `"echo"` を受信
  - 統合: selected_protocol が含有しない → `Error::InvalidArgument`
  - 統合: selected_protocol = `None` + WT-Available-Protocols 不在 → 正常受理 (既存テストの後方互換)
- CHANGES.md `## develop` に `[CHANGE]` エントリを追加し、draft-ietf-webtrans-http2-15 Section 3.3 準拠の `accept()` シグネチャ拡張と WT-Available-Protocols / WT-Protocol サポートを記載すること

## 解決方法

- `src/webtransport/protocols.rs` を新規追加し、`WtAvailableProtocols::parse` (RFC 8941 List of String) と `serialize_wt_protocol` (sf-string) を実装した。パース失敗は field 無視用の `invalid_input`。
- `WtServerRequest::accept` に `selected_protocol: Option<&[u8]>` を追加し、クライアント一覧含有検証後に `wt-protocol` レスポンスヘッダーを付与する。`wt_available_protocols()` helper と `selected_protocol()` アクセサも追加した。
- Sans I/O 単体テスト (`tests/test_webtransport/protocols.rs`) と tokio-http2 統合テスト (選択成功 / リスト外拒否) を追加した。
- draft-15 残り対応と同じブランチ `feature/change-wt-draft15-remaining` で実装した。

## 参照仕様

- draft-ietf-webtrans-http2-15 Section 3.3 (Application Protocol Negotiation), L366-L383
- draft-ietf-webtrans-http3-15 Section 3.3 (Application Protocol Negotiation), L531-L581 — WT-Available-Protocols / WT-Protocol の実定義
- RFC 8941 Section 3.1 (Lists)
- RFC 8941 Section 3.3 (Items)
- RFC 8941 Section 3.3.3 (Strings) — sf-string 値域 0x20-0x7E、`"` と `\` のエスケープ
- RFC 8941 Section 4.1.6 (Serializing a String)
- RFC 8941 Section 4.2.1 (Parsing a List)
- RFC 8941 Section 4.2.5 (Parsing a String)
- RFC 9110 Section 15.5.7 (406 Not Acceptable) — 呼び出し側が `reject(406)` を選ぶ場合の参照

## 依存関係

- 前提: 0062 (Origin 検証、closed)、0063 (TLS バージョン要件チェック、closed)、0064 (WebTransport-Init、closed) — `accept()` 内処理順序は 0063 設計判断 5 で確定済み
- 関連: 0074 (refs/ の draft 番号同期、open) — 0074 が先にマージされると新規ファイル (`protocols.rs`) のコメントも置換対象になる。0074 が未完了でも本 issue の実装は可能だが、新規ファイルのコメントは draft-15 で記述すること
- 実装順序: 0063 (完了) → 0064 (完了) → 0066
