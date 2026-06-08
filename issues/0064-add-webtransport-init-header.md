# WebTransport-Init ヘッダーフィールドのパースと SETTINGS マージが未実装

- Priority: High
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/add-webtransport-init-header

## 目的

draft-ietf-webtrans-http2-14 Section 4.3.2 に定義されている WebTransport-Init ヘッダーフィールドをパースし、同 Section 4.3 (L480-L485) の MUST 規則に従って SETTINGS 値とマージした初期フロー制御値を決定する機能を追加する。

## 優先度根拠

SETTINGS 経由のフロー制御初期値交換は既に完備しているが、WebTransport-Init は SETTINGS と並ぶ正規の初期値交換手段として仕様定義されており、ピアが同ヘッダーを送信した場合に無視すると相互運用性に支障が出る。また仕様上 MUST でマージ規則が規定されている。

## 現状

draft-ietf-webtrans-http2-14 Section 4.3 (L480-L485):

> If both the SETTINGS and the header field are present when a WebTransport session is established, the endpoint MUST use the greater of the two values for each corresponding initial flow control value.

同 Section 4.3.2 (L521-L541) で WebTransport-Init は Dictionary Structured Field (RFC 8941 Section 3.2) と定義されている。以下のキーが定義されている:

| キー | 意味 | 対応する WtConfig フィールド |
|------|------|------------------------------|
| `u` (Integer) | 単方向ストリーム初期フロー制御上限 (受信者向け) | `initial_max_stream_data_uni` |
| `bl` (Integer) | 双方向ストリーム初期フロー制御上限 (送信者向け) | `initial_max_stream_data_bidi_remote` (自身から見てピア発起) |
| `br` (Integer) | 双方向ストリーム初期フロー制御上限 (受信者向け) | `initial_max_stream_data_bidi_local` (自身から見て自身発起) |

現在の実装: `src/` 以下に WebTransport-Init に関するコードは一切存在しない。

## 完了条件

- WebTransport-Init ヘッダーが RFC 8941 Dictionary として正しくパースできること
- `u`, `bl`, `br` キーが正しく抽出され、Integer でない場合はパースエラーになること
- 未知キーが無視されること
- パース失敗時に 4xx (400 Bad Request) で拒否されること
- SETTINGS 値と正しくマージ（各フィールドの大きい方を使う）されること
- パース結果が `WtConfig` に反映されること
- 単体テストで検証されていること

### 本 issue のスコープ外

- `initial_max_data` (セッションレベル) と `initial_max_streams_*` (ストリーム数) は WebTransport-Init で定義されていないため、本 issue の対象外。既存の SETTINGS 経由の値のみで決定される。
- Fuzzing: RFC 8941 パーサーへの任意バイト列入力に対するパニック耐性は、パーサー実装方針（自前実装 or 外部クレート）が決まった後に別途対応する。

## 解決方法

### RFC 8941 パーサーの実装方針

RFC 8941 Section 3.2 の Dictionary は OWS、パラメータ、ベアアイテム、クオート/エスケープ処理を含む完全な文法を持つ。簡易パース（カンマ＋イコール分割）では仕様準拠できない。

2 つの実装方針がある:

1. **`sfv` クレートを使用**: RFC 8941 の完全実装。依存追加になるが、仕様適合性は高い。
2. **自前実装**: 依存最小化方針に沿うが、RFC 8941 の完全準拠実装は数週間規模の作業になる。

プロジェクトの依存最小化方針を考慮し、当面は自前の必要最小限パーサー（本 issue で必要な Dictionary と Integer 値の抽出のみ）で対応する。RFC 8941 の全機能を実装する必要はなく、WebTransport-Init ヘッダーに出現しうる形式に限定したパーサーで十分。

### Sans I/O 層

`WtConfig` に WebTransport-Init のパース結果から構築するメソッドを追加:

```rust
impl WtConfig {
    /// WebTransport-Init ヘッダー値から WtConfig を構築する
    ///
    /// draft-ietf-webtrans-http2-14 Section 4.3.2:
    /// Dictionary Structured Field (RFC 8941 Section 3.2)
    pub fn from_webtransport_init(value: &[u8]) -> Result<WtConfig, WtError> {
        // RFC 8941 Dictionary パース
        // u → initial_max_stream_data_uni
        // bl → initial_max_stream_data_bidi_remote
        // br → initial_max_stream_data_bidi_local
        // 未知キーは無視
        // パース失敗は WtError で返す
    }

    /// SETTINGS 値と WebTransport-Init 値をマージする
    ///
    /// draft-ietf-webtrans-http2-14 Section 4.3 (L480-L485):
    /// 両方ある場合は大きい方を使う (MUST)
    pub fn merge_with_settings(&mut self, settings: &WtConfig) {
        self.initial_max_stream_data_uni =
            self.initial_max_stream_data_uni.max(settings.initial_max_stream_data_uni);
        self.initial_max_stream_data_bidi_local =
            self.initial_max_stream_data_bidi_local.max(settings.initial_max_stream_data_bidi_local);
        self.initial_max_stream_data_bidi_remote =
            self.initial_max_stream_data_bidi_remote.max(settings.initial_max_stream_data_bidi_remote);
    }
}
```

注意: `merge_with_settings` は `&mut self` を取り、自身（WebTransport-Init 由来）に引数（SETTINGS 由来）の大きい方を適用する。Sans I/O 層が SETTINGS 値を直接知ることはなく、呼び出し側が明示的に値を渡す。

### tokio-http2 層

`WtServerRequest::accept()` で WebTransport-Init ヘッダーを検索し、存在すればパースして SETTINGS 値とマージする。これには `ServerConnection` から SETTINGS 値を取得する API が必要だが、本 issue のスコープが大きくなるため、**マージロジックの呼び出し側への提供に留め、`accept()` 内での自動マージは後続 issue (0110 等) で対応する**。

暫定対応として:
1. `WtServerRequest` に `webtransport_init()` メソッド追加（ヘッダー値の取得とパース）
2. `WtConfig` に `from_webtransport_init` と `merge_with_settings` 追加
3. 呼び出し側が以下のフローで使用:
```rust
let mut config = WtConfig::default();
if let Some(init_value) = req.webtransport_init()? {
    config = WtConfig::from_webtransport_init(init_value)?;
    config.merge_with_settings(&settings_config);
}
req.accept(config, allowed_origin).await?;
```

### 他 issue との競合

- 0062 (Origin 検証): 本 issue より先に実装済みの前提。`accept()` のシグネチャは `accept(config, allowed_origin)`。
- 0066 (サブプロトコル): 本 issue より後に実装する前提。

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 4.3 (Initial Flow Control Limits), L465-L486
- draft-ietf-webtrans-http2-14 Section 4.3.2 (Flow Control Header Field), L519-L541
- RFC 8941 Section 3.2 (Dictionary Structured Fields)
