# Origin ヘッダー検証機構の追加

- Priority: High
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/add-origin-header-verification

## 目的

draft-ietf-webtrans-http2-14 Section 3.2 の MUST 要件に従い、WebTransport CONNECT リクエストの Origin ヘッダーをサーバー側で検証する機構を追加する。任意の origin からの接続を受け入れないようにする。

## 優先度根拠

仕様上の MUST 要件違反。Origin ヘッダーの検証がないと、任意の origin からの WebTransport 接続を受け入れてしまい、セキュリティ上の問題となる。Origin ヘッダーの形式は RFC 6454 Section 7 で定義される。

## 現状

draft-ietf-webtrans-http2-14 Section 3.2 (L290-L301):

> In a Web context, the request MUST include an Origin header field [ORIGIN] that includes the origin of the site that requested the creation of the session.
>
> The WebTransport server MUST verify the Origin header to ensure that the specified origin is allowed to access the server in question.

現在の実装:

- `crates/tokio-http2/src/webtransport.rs:87-90` — `WtServerRequest::origin()` メソッドで Origin ヘッダー値を取得可能。しかし検証ロジックはなく、呼び出し側に委ねられている。
- `crates/tokio-http2/src/webtransport.rs:103-154` — `WtServerRequest::accept()` は Origin の値を見ずに無条件で 200 を返す。
- 検証を怠った場合、任意の origin からの WebTransport 接続を受け入れてしまう。

## 完了条件

- `accept()` に許可する origin を指定できること
- 許可された origin と一致する場合は 200 で受理されること
- 許可された origin と一致しない場合は 403 で拒否されること
- Origin ヘッダーが存在しない場合も 403 で拒否されること（Web context の MUST 要件）
- 許可 origin 未指定の場合は検証をスキップできること（非 Web context 向け）
- 既存の `reject()` の呼び出し側に影響を与えないこと（`accept()` のシグネチャ変更に留まる）
- 単体テストで検証されていること

### 非 Web context の扱い

仕様は「In a Web context」に限定して Origin を MUST としている。サーバー間通信や非ブラウザクライアントなど、Web context でないケースでは Origin が存在しない可能性がある。本実装では `allowed_origin: None` を指定することで検証をスキップできるようにし、呼び出し側がコンテキストを判断する。

## 解決方法

### tokio-http2 層 (`crates/tokio-http2/src/webtransport.rs`)

`WtServerRequest::accept()` のシグネチャと実装を変更する。

```rust
pub async fn accept(
    self,
    config: WtConfig,
    allowed_origin: Option<&[u8]>,
) -> Result<WtServerSession> {
    // draft-ietf-webtrans-http2-14 Section 3.2:
    // Web context では Origin ヘッダーを MUST verify する。
    // Origin の形式は RFC 6454 Section 7 で定義される。
    if let Some(allowed) = allowed_origin {
        let actual = self.origin().ok_or_else(|| {
            Error::InvalidArgument("Origin header is required but missing".into())
        })?;
        // RFC 6454 Section 7: Origin = scheme "://" host [ ":" port ]
        // ASCII case-insensitive で比較する
        if !actual.eq_ignore_ascii_case(allowed) {
            // HTTP 403 Forbidden
            let status_str = "403";
            let response = vec![
                HeaderField::new(":status", status_str)
                    .expect("3-digit numeric status produces a valid :status header"),
            ];
            self.conn
                .send_response(stream_id, response, true)
                .await?;
            return Err(Error::InvalidArgument(format!(
                "origin rejected: allowed={}, actual={}",
                String::from_utf8_lossy(allowed),
                String::from_utf8_lossy(actual),
            )));
        }
    }

    // 既存の accept ロジック（200 レスポンス + WtSession 生成）
    // ...
}
```

**設計判断**: 本 issue の修正対象は `WtServerRequest::accept()` のみ。Sans I/O 層 (`WtConfig`, `WtSession`) への変更は不要。Origin は HTTP 層の関心事であり、Sans I/O 原則に従い tokio-http2 層で完結させる。

**他 issue との競合**: 0064 (WebTransport-Init) と 0066 (サブプロトコル) も `accept()` のシグネチャ変更を必要とする。本 issue では `allowed_origin` パラメータのみを追加し、後続 issue でさらにパラメータが追加される前提とする。実装順序は 0062 → 0064 → 0066 を推奨。

#### エッジケース

- **Origin が空バイト列**: 一致しないため 403 で拒否される。
- **Origin のスキーム部の大文字小文字**: `eq_ignore_ascii_case` で比較するため、`HTTPS://example.com` と `https://example.com` は一致扱い。
- **Origin に port が含まれる/含まれない**: RFC 6454 に従い、`https://example.com:443` と `https://example.com` は異なる origin として扱われる。呼び出し側が適切な正規化を行うことを推奨する。

### テスト戦略

#### 単体テスト (`crates/tokio-http2/tests/test_webtransport.rs`)

- `allowed_origin=Some(b"https://example.com")`, Origin 一致 → accept 成功
- `allowed_origin=Some(b"https://example.com")`, Origin 不一致 → accept エラー
- `allowed_origin=Some(b"https://example.com")`, Origin 不在 → accept エラー
- `allowed_origin=None` → accept 成功（検証スキップ）
- Origin の ASCII case-insensitive 比較の確認

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 3.2 (Creating a New Session), L270-L315
- RFC 6454 Section 7 (The Web Origin Concept — Origin Header)
