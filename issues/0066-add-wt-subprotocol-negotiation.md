# WT-Available-Protocols / WT-Protocol サブプロトコルネゴシエーション未実装

- Priority: Medium
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/add-wt-subprotocol-negotiation

## 目的

draft-ietf-webtrans-http2-14 Section 3.3 に定義されているサブプロトコルネゴシエーション機能 (WT-Available-Protocols / WT-Protocol ヘッダー) を実装する。

## 優先度根拠

仕様上の MAY 要件。ALPN ライクなサブプロトコルネゴシエーションは既存プロトコルの WebTransport 移植に有用だが、必須ではない。Priority は Medium とする。

## 現状

draft-ietf-webtrans-http2-14 Section 3.3 (L317-L342):

> The user agent MAY include a WT-Available-Protocols header field in the CONNECT request. The WT-Available-Protocols enumerates the possible protocols in preference order. If the server receives such a header, it MAY include a WT-Protocol field in a successful (2xx) response. If it does, the server MUST include a single choice from the client's list in that field. Servers MAY reject the request if the client did not include a suitable protocol.
>
> Both WT-Available-Protocols and WT-Protocol are defined in Section 3.4 of [WEBTRANSPORT-H3].

現在の実装:

- `WT-Available-Protocols` も `WT-Protocol` も定義されていない
- `WtServerRequest` はヘッダーを保持している (`self.headers`) が、`..` で破棄されているため `accept()` 内でのアクセスは `self.conn` を move する前に `self.headers` から抽出する必要がある
- `send_response` は現在 `:status=200` のみ。WT-Protocol を追加するにはレスポンスヘッダーリストを動的構築する必要がある

## 前提条件

**WEBTRANSPORT-H3 仕様が `refs/` に存在しない。** WT-Available-Protocols / WT-Protocol の正確なフォーマット（区切り文字、エンコーディング、複数値表現）は WEBTRANSPORT-H3 Section 3.4 で定義されている。本 issue の実装着手前に以下が必要:

1. `refs/` に `draft-ietf-webtrans-http3-14.txt` を追加する
2. Section 3.4 のフォーマット定義を確認する

## 解決方法

### 実装設計

```rust
// WtServerRequest に追加
pub fn wt_available_protocols(&self) -> Option<Vec<&[u8]>> {
    // "wt-available-protocols" ヘッダーを検索し、フォーマット定義に従って分割
}

// WtServerSession に追加
pub fn selected_protocol(&self) -> Option<&str> {
    // サーバーが選択したプロトコル（WT-Protocol で返した値）
}

// accept() のシグネチャ
pub async fn accept(
    self,
    config: WtConfig,
    allowed_origin: Option<&[u8]>,
    selected_protocol: Option<&str>,
) -> Result<WtServerSession>
```

### 処理フロー

1. `accept()` 呼び出し前に `wt_available_protocols()` でクライアント提示リストを取得
2. 呼び出し側がリストからプロトコルを選択（または拒否）
3. `accept()` に選択結果を `selected_protocol: Some(protocol)` で渡す
4. `accept()` 内で `selected_protocol` がクライアントリストに存在することを検証 (MUST)
5. `:status=200` レスポンスに `WT-Protocol` ヘッダーを含める
6. `WtServerSession.selected_protocol()` で選択結果を後段から参照可能にする

### 0062/0064 との accept() シグネチャ競合

3 issue の統合後の `accept()` シグネチャは以下の順序で拡張される:

0062: `accept(self, config, allowed_origin)`
0064: `accept(self, config, allowed_origin)` （WtConfig 経由で解決、シグネチャ不変）
0066: `accept(self, config, allowed_origin, selected_protocol)`

実装順序: 0062 → 0064 → 0066

### エッジケース

- **WT-Available-Protocols 存在 + selected_protocol=None**: サブプロトコルなしで受理
- **selected_protocol 指定 + WT-Available-Protocols 不在**: クライアントリスト不在だが selected_protocol を返すのは仕様違反。エラーにするか考慮が必要
- **selected_protocol がクライアントリストに含まれていない**: MUST 違反。エラーを返す
- **空の WT-Available-Protocols**: フォーマット定義に依存
- **拒否パス**: 適切なプロトコルがない場合、406 (Not Acceptable) で拒否

### テスト戦略

単体テスト (`crates/tokio-http2/tests/test_webtransport.rs`):
- WT-Available-Protocols 正常パース
- WT-Protocol ヘッダーがレスポンスに含まれること
- selected_protocol がクライアントリスト外の場合のエラー

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 3.3 (Application Protocol Negotiation), L317-L342
- WEBTRANSPORT-H3 Section 3.4 (refs/ 未収録。実装着手前に追加が必要)
