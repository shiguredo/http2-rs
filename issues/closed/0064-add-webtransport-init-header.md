# WebTransport-Init ヘッダーフィールドのパースと SETTINGS マージを追加する

- Priority: High
- Created: 2026-06-08
- Polished: 2026-06-09
- Completed: 2026-06-09
- Model: deepseek-v4-pro
- Branch: feature/add-webtransport-init-header

## 目的

draft-ietf-webtrans-http2-14 Section 4.3.2 に定義されている WebTransport-Init ヘッダーフィールド (RFC 8941 Dictionary Structured Field) をパースし、同 Section 4.3 (L480-L483) の MUST 規則に従って SETTINGS 値とマージした初期フロー制御値を決定する機能を追加する。

## 優先度根拠

SETTINGS 経由のフロー制御初期値交換は既に完備しているが、WebTransport-Init は SETTINGS と並ぶ正規の初期値交換手段として仕様定義されており、ピアが同ヘッダーを送信した場合に無視すると相互運用性に支障が出る。また仕様上 MUST でマージ規則・拒否規則 (パース失敗時 4xx) が規定されている。

## 現状

draft-ietf-webtrans-http2-14 Section 4.3 (L480-L483):

> If both the SETTINGS and the header field are present when a WebTransport session is established, the endpoint MUST use the greater of the two values for each corresponding initial flow control value.

同 Section 4.3.2 (L521-L541):

> The WebTransport-Init HTTP header field can be used to communicate the initial values of the flow control windows, similar to how QUIC uses transport parameters. The WebTransport-Init is a Dictionary Structured Field (Section 3.2 of [RFC8941]). If the WebTransport-Init field cannot be parsed correctly or does not have the correct type, the endpoint MUST reject the CONNECT request with a 4xx status code.

以下のキーが定義されている (L530-L537):

| キー | 仕様文言の意味 | 対応する `WtConfig` フィールド |
|------|----------------|--------------------------------|
| `u` (Integer) | unidirectional streams opened by the recipient of this header field | `initial_max_stream_data_uni` |
| `bl` (Integer) | bidirectional streams opened by the sender of this header field | `initial_max_stream_data_bidi_remote` |
| `br` (Integer) | bidirectional streams opened by the recipient of this header field | `initial_max_stream_data_bidi_local` |

`bl`/`br` の対応根拠: サーバーが受信した場合、仕様の sender = ピア、recipient = 自身。`WtConfig::initial_max_stream_data_bidi_local` は「自身が開始したストリーム」、`_bidi_remote` は「ピアが開始したストリーム」(`src/webtransport/mod.rs` L90-L99 の docstring) なので、`bl` (sender が開く bidi) → `_bidi_remote`、`br` (recipient が開く bidi) → `_bidi_local` となる。

仕様 L539-L540: 「If any of these keys are present but contain invalid values, the endpoint MUST reject the CONNECT request with a 4xx status code.」

仕様 L541: 「Unknown keys and parameters in the dictionary MUST be ignored.」(known キーに付随する未知パラメータも含む)

現在の実装: `grep -rn "WebTransport-Init\|webtransport-init\|sfv\|8941" src/ crates/` でヒットせず、RFC 8941 Dictionary パーサーも `WebTransport-Init` 処理も完全に未実装。

## 本 issue で扱う

- Sans I/O 層: `WtInit` 構造体と `WtInit::parse(value: &[u8]) -> Result<WtInit, WtError>` (RFC 8941 Dictionary 必要最小限パーサー)、および `WtConfig::apply_init(&mut self, init: &WtInit)` (max マージ) を追加する
- tokio-http2 層: `WtServerRequest::webtransport_init(&self) -> Option<&[u8]>` (生バイト列 helper) を追加し、`WtServerRequest::accept()` 内で Origin 検証の後に自動パース・マージ・拒否を行う
- 単体テストで境界値・未知キー・パース失敗・マージ結果を検証する

## 本 issue のスコープ外

- **`initial_max_data` / `initial_max_streams_bidi` / `initial_max_streams_uni` の Init 経由更新**: WebTransport-Init のキーには定義されていない (Section 4.3.2 L527-L537 で `u`/`bl`/`br` の 3 つのみ)。`WtConfig::apply_init` はこれらに触れない
- **クライアント側 (Init を送る側) の SHOULD 要件** (Section 4.3 L483-L485 「SHOULD ensure that the header field values are greater than or equal to the values provided in the SETTINGS」): クライアント側 WebTransport API が現状 `crates/tokio-http2/src/client.rs` に存在しないため、クライアント側 API 追加時に併せて対応する
- **`sfv` クレートなど外部依存の追加**: プロジェクトの依存最小化方針 (CLAUDE.md / shiguredo-rust 規約) に従い、必要最小限の自前パーサーで対応する
- **Fuzzing**: RFC 8941 パーサーへの任意バイト列入力に対するパニック耐性検証は、本 issue とは別途 `fuzz/` ディレクトリに追加する
- **WebTransport-Init の SETTINGS との不整合 SHOULD 警告**: 仕様 L483-L485 は送信側 SHOULD で受信側挙動は規定されていない

## 設計判断

### 1. `WtInit` を 3 値の Optional 構造体として導入する

`from_webtransport_init` の戻り値を `WtConfig` にすると、Init に存在しないフィールド (`initial_max_data` 等) が `WtConfig::default()` 値 (1 MiB 等) で埋まり、後段の max マージで SETTINGS 由来の小さい値を上書きしてしまう (仕様 L480-L483 MUST 違反)。これを避けるため、Init 由来 3 値だけを持つ専用構造体を導入する:

```rust
// src/webtransport/init.rs (新規)
#[derive(Debug, Default, Clone)]
pub struct WtInit {
    pub u: Option<u64>,
    pub bl: Option<u64>,
    pub br: Option<u64>,
}
```

`Option<u64>` でキー不在を表現する。`u64` を採用するのは `WtConfig` フィールドが `u64` で、かつ RFC 8941 Integer の正値上限 999_999_999_999_999 が `u64::MAX` 内に収まるため。

### 2. マージは `WtConfig::apply_init` の in-place で行う

`accept()` 呼び出し側は SETTINGS 由来の `WtConfig` を `config` 引数で渡す。`accept()` 内で WebTransport-Init をパースしたら、`config.apply_init(&init)` で `u`/`bl`/`br` 各キーが `Some(_)` の場合のみ `max` を取って `config` を上書きする:

```rust
impl WtConfig {
    pub fn apply_init(&mut self, init: &WtInit) {
        // draft-ietf-webtrans-http2-14 Section 4.3 L480-L483: MUST use the greater of the two values
        if let Some(u) = init.u {
            self.initial_max_stream_data_uni = self.initial_max_stream_data_uni.max(u);
        }
        if let Some(bl) = init.bl {
            self.initial_max_stream_data_bidi_remote = self.initial_max_stream_data_bidi_remote.max(bl);
        }
        if let Some(br) = init.br {
            self.initial_max_stream_data_bidi_local = self.initial_max_stream_data_bidi_local.max(br);
        }
    }
}
```

Init 不在のキーは触らないため、`config` の他フィールドや既存値は保たれる。

### 3. `accept()` 内で自動的に 4xx 送信して拒否する

仕様 (L525-L526, L539-L540) は「endpoint MUST reject the CONNECT request with a 4xx status code」を要求する。Origin 検証 (0062、`webtransport.rs` L125-L139) が `accept()` 内で 403 を自動送信するパターンと統一し、本 issue でもパース失敗時は `accept()` 内で `:status=400` を END_STREAM 付きで送信してから `Err` を返す。

`accept()` シグネチャは変更しない (引数追加なし)。0064 の挿入は 0063 設計判断 5 で示した「3. (将来 0064 で追加: WebTransport-Init パース)」位置 (= Origin 検証の後ろ、`:status=200` 送信の前)。

### 4. `webtransport_init()` helper の戻り値型

`WtServerRequest::webtransport_init(&self) -> Option<&[u8]>` は既存の `header()` (`webtransport.rs` L92-L97) と同型の生バイト列 helper として公開する。パース処理は `accept()` 内部のみで行う (公開 API としてのパース helper は不要)。`Option<&[u8]>` のライフタイムは `&self` に紐づくため、`accept()` 内では部分ムーブ前に `to_vec()` してから処理する。

### 5. WtError バリアントは既存の `InvalidInput` を再利用する

`WtError` (`src/webtransport/error.rs` L9-L37) に `InvalidInput` バリアントが既存。RFC 8941 パースエラー (型不一致・15 桁超過・負値・重複ヘッダー連結失敗など) は `WtError::invalid_input(...)` を `reason` 文字列で原因を分けて返す。新規バリアント追加は本 issue のスコープ外。

### 6. 複数 `webtransport-init` ヘッダーの取り扱い

HTTP/2 では同一 field name のヘッダーが複数回現れる可能性がある。RFC 8941 Section 4.2 (L1042-L1046) は「parsers MUST combine all field lines in the same section that case-insensitively match the field name into one comma-separated field-value」と規定するが、現状の HPACK 経由ではアプリケーションが意図的に送らない限り複数行は発生しない。本 issue では `webtransport_init()` helper を **最初の 1 個のみ** を返す素朴な実装とし、複数行結合は将来必要になれば別 issue で対応する。

## 完了条件

- `src/webtransport/init.rs` (新規) に `pub struct WtInit { pub u: Option<u64>, pub bl: Option<u64>, pub br: Option<u64> }` と `WtInit::parse(value: &[u8]) -> Result<WtInit, WtError>` が追加されていること
- `src/webtransport/mod.rs` に `pub mod init;` と `pub use init::WtInit;` が追加され、`shiguredo_http2::webtransport::WtInit` で参照可能なこと
- `WtConfig::apply_init(&mut self, init: &WtInit)` が追加され、`u`/`bl`/`br` の `Some(_)` 値のみ `max` で上書きすること
- `WtServerRequest::webtransport_init(&self) -> Option<&[u8]>` が追加され、`b"webtransport-init"` (HTTP/2 lowercase) と一致するヘッダーの値を返すこと
- `WtServerRequest::accept()` が Origin 検証の後に `webtransport_init()` を呼び、`Some(_)` なら `WtInit::parse` → `config.apply_init` の順に処理し、パース失敗時は `:status=400` を END_STREAM 付きで送信してから `Err(Error::InvalidArgument(...))` を返すこと
- RFC 8941 準拠の以下のケースが `WtInit::parse` で検証されていること:
  - 正常系: `u=100, bl=200, br=300` でそれぞれ `Some(100)`, `Some(200)`, `Some(300)` を返す
  - 未知キー無視: `u=100, x=999` で `u=Some(100)`、`x` は無視
  - パラメータ無視 (known/unknown 問わず): `u=100;foo=bar` で `u=Some(100)`、`;foo=bar` は無視
  - Integer 範囲 (RFC 8941 §3.3.1, L530-L554): 16 桁以上の数字列 (例: `u=1000000000000000`) は `WtError::invalid_input` を返す
  - 負値 (RFC 8941 §3.3.1 は signed Integer を許容するが、フロー制御値として無効): `u=-1` は `WtError::invalid_input` を返す
  - 型不一致 (RFC 8941 §3.3 以外の bare item): `u=?1` (Boolean) / `u="abc"` (String) / `u=:YWJj:` (Byte Sequence) / `u=tok` (Token) のいずれも `WtError::invalid_input` を返す
  - 重複キー (RFC 8941 §4.2.2 L1212-L1213): `u=10, u=20` は last-wins で `u=Some(20)` を返す
  - 空 Dictionary: 空文字列で `WtInit::default()` (全 `None`) を返す
- `WtServerRequest::accept()` レベルの単体テストで以下が検証されていること (`crates/tokio-http2/tests/test_webtransport.rs`):
  - WebTransport-Init `u=N` (`N` は `WtConfig::default().initial_max_stream_data_uni = 262_144` より大きい値) を送ると、セッション確立後の uni ストリーム初期最大データ量に `N` が反映される
  - WebTransport-Init `u=N` (`N` がデフォルトより小さい値) を送ると、SETTINGS / `config` 由来のデフォルト値が維持される
  - WebTransport-Init パース失敗 (例: `u=-1`) で `:status=400` レスポンスが返り、CONNECT ストリームが END_STREAM で閉じられる
- CHANGES.md `## develop` に `[ADD]` エントリを追加し、`WtInit` / `WtConfig::apply_init` / `WtServerRequest::webtransport_init` の追加と `accept()` 内自動マージを記載すること (公開 API 追加のみで既存シグネチャ不変のため `[ADD]`)

## 解決方法

- `src/webtransport/init.rs` を新規追加し、`pub struct WtInit { pub u: Option<u64>, pub bl: Option<u64>, pub br: Option<u64> }` と `WtInit::parse(value: &[u8]) -> Result<WtInit, WtError>` を実装。RFC 8941 §4.2 系の Dictionary パーサーを必要最小限で自前実装 (外部依存ゼロ)。known キー以外は値型を識別して読み飛ばし、known キーで Integer 以外/負値/16 桁以上などはすべて `WtError::invalid_input` を返す。
- `src/webtransport/mod.rs` に `pub mod init;` / `pub use init::WtInit;` を追加し、`WtConfig::apply_init(&mut self, init: &WtInit)` を実装。`u`/`bl`/`br` の `Some(_)` 値のみ `max` で上書きする。docstring に「WebTransport-Init を受信した側 (recipient) の `WtConfig` 用」と明記し、sender/recipient の解釈逆転で誤用しないよう注意を残した。
- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest` に `webtransport_init() -> Option<&[u8]>` を追加。`accept(mut self, mut config: WtConfig, allowed_origin: Option<&[u8]>)` の Origin 検証の後ろで自動的に `WtInit::parse` → `config.apply_init(&init)` を実行し、パース失敗時は `:status=400` を END_STREAM 付きで送って `Err(Error::InvalidArgument(...))` を返す。
- Sans I/O 単体テスト 25 件を `tests/test_webtransport/init.rs` に追加 (issue 完了条件のパース系ケース全件 + `apply_init` の伝搬テストとして `WtSession::server(config)` 経由で uni/bidi ストリームの `send_available()` が反映されることを検証)。
- tokio-http2 統合テスト 3 件を `crates/tokio-http2/tests/test_webtransport.rs` に追加 (大きい値で `accept` 成功、小さい値で `accept` 成功、`u=-1` で `:status=400` 拒否 + END_STREAM)。
- 既知の制限: 同名 `webtransport-init` ヘッダーが複数あった場合の RFC 8941 §4.2 comma-concat 結合は未対応。最初の 1 個のみを参照する。コメントに明記し将来の別 issue で扱う。
- `CHANGES.md` の `## develop` に `[ADD]` エントリを追加。

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 4.3 (Initial Flow Control Limits), L465-L486 (受信側 MUST は L480-L483)
- draft-ietf-webtrans-http2-14 Section 4.3.2 (Flow Control Header Field), L519-L541
- RFC 8941 Section 3.2 (Dictionary Structured Fields)
- RFC 8941 Section 3.3.1 (Integers) — 値範囲と 15 桁制限 (refs/rfc8941.txt L530-L554)
- RFC 8941 Section 4.2.2 (Parsing a Dictionary) — 重複キー last-wins (L1212-L1213)
- RFC 8941 Section 4.2.4 (Parsing an Integer or Decimal) — 16 桁目で fail (L1319-L1380)

## 依存関係

- 前提: 0062 (Origin 検証、closed)、0063 (TLS バージョン要件チェック) — `accept()` 内の処理順序は 0063 設計判断 5 で「TLS → Origin → 0064 (本 issue) → :status=200」と確定済み
- 後続: 0066 (サブプロトコルネゴシエーション) が `accept()` 内 0064 処理の後ろにサブプロトコル検証を挿入する想定 (0066 内で「実装順序: 0062 → 0064 → 0066」と明記済み)
- 実装順序: 0063 → 0064 → 0066
