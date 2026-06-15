# サーバー側 TLS Keying Material Exporter API を追加する

- Priority: Medium
- Created: 2026-06-08
- Polished: 2026-06-15
- Model: deepseek-v4-pro
- Branch: feature/add-tls-keying-material-exporter

## 目的

draft-ietf-webtrans-http2-14 Section 5.3 (L683-L708) の条件付き SHALL 要件に従い、アプリケーションが要求した場合に `EXPORTER-WebTransport` ラベルとセッション固有の Exporter Context を用いて TLS exporter を導出できるサーバー側 API を追加する。本 issue ではサーバー側 API のみを対象とし、クライアント側 API はスコープ外とする。

## 優先度根拠

WebTransport over HTTP/2 が TLS exporter をサポートする仕様上の SHALL 要件は条件付きであり、アプリケーションが exporter を要求した場合に発動する。セキュアなセッション固有の鍵素材が必要なプロトコル (例: WebTransport over QUIC からの移植プロトコル) では必須となる。一方、多くの WebTransport ユースケースで毎回必要ではないため Priority は Medium とする。

## 現状

draft-ietf-webtrans-http2-14 Section 5.3 L689-L694:

> If the application requests an exporter for a given WebTransport session with a specified label and context, the resulting exporter SHALL be a TLS exporter as defined in Section 7.5 of [TLS] with the label set to "EXPORTER-WebTransport" and the context set to the serialization of the "WebTransport Exporter Context" struct as defined below.

```
WebTransport Exporter Context {
  WebTransport Session ID (64),
  WebTransport Application-Supplied Exporter Label Length (8),
  WebTransport Application-Supplied Exporter Label (8..),
  WebTransport Application-Supplied Exporter Context Length (8),
  WebTransport Application-Supplied Exporter Context (..)
}
```

L706-L708 (Context omission):

> A TLS exporter API might permit the context field to be omitted. In this case, as with TLS 1.3, the WebTransport Application-Supplied Exporter Context becomes zero-length if omitted.

サイズ表記の解釈 (RFC 9000 Section 1.3 の慣用に基づく):

- `(64)` = 64 bit fixed (8 バイト)
- `(8)` = 8 bit fixed Length フィールド (u8 で表現、値域 0-255)
- `(8..)` / `(..)` = 可変長コンテンツ。直前の 8 bit Length フィールドで長さを表現するため最大 255 バイト

注: RFC 9000 Section 1.3 (`refs/rfc9000.txt` L451-L454) を厳密に読むと `(8..)` は「最小 8 bit (= 1 バイト) 以上」を意味し、`(..)` は「最小 0 bit 以上」を意味する。よって厳密解釈では Application-Supplied Exporter Label (`(8..)`) は最低 1 バイトとなるが、本実装では Length フィールド (8 bit、値 0 を含む) を権威として扱い、Application-Supplied Exporter Label も空 (Length=0) を許容する。根拠:
  - draft Section 5.3 L706-L708 (Context omission) は Application-Supplied Exporter Context (`(..)`) のみが zero-length を取り得ると明示しているが、`(8..)` の zero-length 禁止は draft 本文に明示されていない
  - Length フィールドが権威であるという他の Capsule (例: WT_CLOSE_SESSION reason) の慣用と整合する
  - draft 本文に表記規則の明示的参照が無い (Notational Conventions セクションが存在しない) ため、QUIC frame をミラーしている (draft L235, L712) ことと RFC 9113 Section 2.2 が RFC 9000 Section 1.3 を採用していることから類推して RFC 9000 Section 1.3 を適用する

Session ID は CONNECT stream ID のことである。draft-ietf-webtrans-http2-14 Section 2 L204-L210:

> The stream that carries the CONNECT request is used to exchange bidirectional data for the session. This stream will be referred to as a _CONNECT stream_. The stream ID of a CONNECT stream, which will be referred to as a _Session ID_, is used to uniquely identify a given WebTransport session within the connection.

Session ID は HTTP/2 Stream ID (RFC 9113 Section 5.1.1、unsigned 31-bit integer、`refs/rfc9113.txt` L904-L907) を `u64` 拡張して 64-bit big-endian で書き込む。draft Section 5.3 自身にはバイト順を明示していないが、RFC 9113 L273 に「All numeric values are in network byte order」と定義されているため、Session ID も big-endian と解釈する。`WebTransport Session ID (64)` の 64-bit 幅は HTTP/3 / QUIC 系の Session ID (62-bit varint) と struct フォーマットを合わせるためのものであり、HTTP/2 stream ID 自体は 31-bit のため上位 33 bit は常に 0 となる。

TLS exporter の定義は RFC 8446 Section 7.5 にある。該当文面:

> The exporter value is computed as:
> ```
> TLS-Exporter(label, context_value, key_length) =
> HKDF-Expand-Label(Derive-Secret(Secret, label, ""),
>                   "exporter", Hash(context_value), key_length)
> ```
> Where Secret is either the early_exporter_master_secret or the exporter_master_secret. Implementations MUST use the exporter_master_secret unless explicitly specified by the application.
>
> If no context is provided, the context_value is zero-length. Consequently, providing no context computes the same value as providing an empty context.
>
> New uses of exporters SHOULD provide a context in all exporter computations, though the value could be empty.

`key_length` は HKDF-Expand-Label 内の `uint16 length` フィールド (RFC 8446 Section 7.1) なので API 上限は 65535 バイトまでだが、HKDF-Expand の反復回数上限 (RFC 5869 Section 2.3) により実用上の最大出力長は `255 * Hash.length` である。rustls もこれを超える要求を拒否する。

## 本 issue で扱う

- Sans I/O 層: `WebTransport Exporter Context` のシリアライズ関数 `serialize_exporter_context(session_id: u64, app_label: &[u8], app_context: &[u8]) -> Result<Vec<u8>, WtError>` を `src/webtransport/exporter.rs` (新規) に追加し、`src/webtransport/mod.rs` から `pub mod exporter;` (`pub mod error;` と `pub mod flow_control;` の間) と `pub use exporter::serialize_exporter_context;` (`pub use error::{...};` と `pub use flow_control::...;` の間) で公開する
- tokio-http2 層:
  - `DriverCmd::ExportKeyingMaterial` を `crates/tokio-http2/src/webtransport.rs` の `DriverCmd` enum (L638-L673) に追加
  - `DriverState::handle_cmd` (L726-L836) に `ExportKeyingMaterial` 処理を追加 (Sans I/O 層でシリアライズ → `ServerConnection::with_tls` 経由で `rustls::ServerConnection::export_keying_material` を呼ぶ)
  - `WtServerSession::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` を `WtServerSession` impl (L283-L371) に追加
  - `WtSessionHandle::export_keying_material` を同じシグネチャで `WtSessionHandle` impl (L406-L455) に追加 (driver 側で `connect_stream_id` から session_id を導出するため、handle 側で session_id を持つ必要はない)
  - `crates/tokio-http2/src/webtransport.rs` の既存 use 文 (L19-L22 付近) に `serialize_exporter_context` を追加

## 本 issue のスコープ外

- **クライアント側 export API**: 現状 `crates/tokio-http2/src/client.rs` に WebTransport クライアント API が無いためサーバー側のみ対応する
- **TLS 1.2 + EMS での export**: draft-ietf-webtrans-http2-14 Section 7 L1425-L1439 は TLS 1.3 または TLS 1.2 + EMS を要求するが、0063 で TLS 1.3 強制が確定しているため本 issue では TLS 1.3 のみを対象とする
- **`refs/rfc8446.txt` / `refs/rfc5869.txt` 新規収録**: これらの RFC 本文は `update-refs` 対象として別途扱う。本 issue では上記の通り必要な文面を引用済み
- **PBT / fuzzing**: `serialize_exporter_context` の入力空間は `app_label` / `app_context` が 255 バイト制限、`length` が 65535 制限、 `session_id` も big-endian 書き込みの境界値をテストすれば十分なため、本 issue では対象外とする

## 設計判断

### 1. シリアライズ関数は Sans I/O 層 (`src/webtransport/exporter.rs` 新規) に配置する

I/O 非依存の純粋なバイト列構築関数なので Sans I/O 層に置く。エラー型は既存 `WtError::invalid_input(reason)` (`src/webtransport/error.rs` L104-L106) で長さ制約違反を表現する。戻り値は `Vec<u8>` (所有データ) とし、サイズが事前に確定するため `Vec::with_capacity(8 + 1 + app_label.len() + 1 + app_context.len())` で 1 回確保する。`pub` とするのは tokio-http2 クレートから `shiguredo_http2::webtransport::serialize_exporter_context` として呼び出すためである (クレート境界を越えるため `pub(crate)` では不可)。

### 2. `app_label` / `app_context` の長さ制約

Length フィールドが 8 bit (= u8) なので、コンテンツは最大 255 バイト。`app_label.len() > 255` または `app_context.len() > 255` のいずれかが成立したら `WtError::invalid_input` でエラーを返す。空スライス (Length=0) は両方とも許容する (draft Section 5.3 L706-L708 の Context omission 規定および現状節の `(8..)` 解釈に従う)。`app_context` の暗黙的切り詰めは行わない (鍵素材分岐の意味が壊れて対向と不一致になるため。issues/closed の `WT_CLOSE_SESSION reason` 切り詰めエラー化 (0061) と同じ方針)。

固定ラベル `"EXPORTER-WebTransport"` は draft-ietf-webtrans-http2-14 Section 5.3 L692-L693 で直接定められている。RFC 8446 Section 7.5 は exporter label の形式要件を RFC 5705 に委ねているが、本実装では draft がラベルを固定しているため RFC 5705 の詳細は間接参照に留まる。`app_label` と `app_context` は draft Section 5.3 が文字セット制約を定めていないため任意のバイト列 (UTF-8 でも非 UTF-8 でも可) を許容する。なお `EXPORTER-WebTransport` ラベル自体は draft-ietf-webtrans-http2-14 / draft-ietf-webtrans-http3-15 共通の固定値であり、RFC 化に伴い変更される可能性がある (その場合は本実装の `EXPORTER_LABEL` 定数も更新する)。

### 3. `length` 検証は driver 側に集約する

`length == 0` および `length > 65535` の検証は tokio-http2 層の `DriverState::handle_cmd` 内で行い、いずれも `Error::InvalidArgument` で弾く。`WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material` 側で先行検証することも可能だが、判定ロジックを 1 箇所に集約してテスト容易性を高めるため driver 側のみに置く。cmd_tx を通したラウンドトリップが必要になり遅延が増えるが、TLS exporter は稀な操作のため許容範囲。

それぞれを tokio-http2 層で先に弾く具体的理由:
- `length == 0`: rustls 0.23 は `Error::General("export_keying_material with zero-length output".into())` で拒否する (`rustls-0.23.40/src/conn.rs`)。この `Error::General(String)` variant に依存すると将来 rustls がメッセージ文字列を変えた際に挙動同定が壊れるため、tokio-http2 層で `Error::InvalidArgument` として先に弾く。意味論的にも「呼び出し側のミス」は `Error::InvalidArgument`、「TLS 層の失敗」は `Error::Tls` という分離を保てる。
- `length > 65535`: HKDF-Expand-Label の `key_length` は RFC 8446 Section 7.1 で `uint16` なので、それを超える値は API 表層で機械的に判定可能。アプリケーション層が誤って巨大値を指定した場合の防御として早期拒否する (`vec![0u8; length]` が巨大メモリを確保することを防ぐ)。

HKDF-Expand の実用上の上限は `255 * Hash.length` (RFC 5869 Section 2.3) であり、TLS 1.3 で選択された hash function 依存 (SHA-256 なら 8160 バイト、SHA-384 なら 12240 バイト) のため事前判定不可。`length` がこの上限を超える場合は rustls から `Error::Tls` が返ることを想定する。

### 5. Session ID は driver タスク内の `DriverState.connect_stream_id` から導出する

Sans I/O 層 `shiguredo_http2::webtransport::WtSession` には CONNECT ストリーム ID を保持するフィールドが無い。tokio-http2 層の `DriverState` (`webtransport.rs` L681-L697) が `connect_stream_id: StreamId` を保持しているため、`u64::from(self.connect_stream_id.as_u32())` で 64-bit 化する (`WtServerRequest::accept()` 内と同じ変換)。Sans I/O 層に HTTP/2 stream ID を持ち込まないことでアーキテクチャ責務を保つ。

`WtServerSession` は既に `session_id: u64` フィールドを保持しているが、本 issue では `DriverCmd::ExportKeyingMaterial` に `session_id` を詰めず driver 側で都度導出する設計を選ぶ。理由は `WtSessionHandle` (session_id を持たない) と挙動を統一し、driver 側を session_id 取得の単一情報源にするため。

本 arm の driver 内処理は `wt_session` (Sans I/O) を一切触らず TLS 直接アクセス (`with_tls`) のみを行う初めての DriverCmd であり、driver の責務範囲は「HTTP/2 と `WtSession` の橋渡し」から「HTTP/2 / `WtSession` / rustls の三者橋渡し」に拡張される。`ServerConnection::with_tls` は 0063 で `pub(crate)` として導入されており、driver 内 (= 常時稼働中の async タスク) で呼ぶ初めての事例となる (0063 では `accept()` 内 = driver spawn 前で 1 度だけ呼んでいた)。

### 6. `WtSessionHandle` 側の重複実装

`WtServerSession` と `WtSessionHandle` は `export_keying_material` メソッドの実装に `cmd_tx: mpsc::UnboundedSender<DriverCmd>` だけが必要であるため、両側で同一の実装になる。既存の `open_bidi` / `open_uni` / `send_datagram` / `drain` も同じ重複パターンを踏襲しており、本 issue もこの流儀に従う。

`WtSessionHandle` には `session_id()` API を追加しない。`into_parts()` の呼び出し側は `WtServerSession::session_id()` で取得した値を別途保持する責務を持つ。将来 `WtSessionHandle::session_id()` API が必要になった場合は別 issue で対応する。

### 7. `accept()` フローへの追加なし

`export_keying_material` は実行時 API なので、`accept()` 内処理 (0063 設計判断 5 で確定した「TLS → Origin → 0064 → 0066 → :status=200」) には何も追加しない。本 issue は driver タスクと公開 API のみを変更する。

### 8. `&self` / `&mut self` の方針

`export_keying_material` は `cmd_tx.send` だけを行うため `&self` で十分である。`WtServerSession` の既存メソッド (`open_bidi` 等) は `&mut self` だが、これらを `&self` に変更すると後方互換を破るため本 issue では既存メソッドを変更しない。今後新規メソッドを追加する際は `&self` で十分なら `&self` とし、段階的に API スタイルを統一する。`WtSessionHandle` は既存メソッドがすべて `&self` であるため、新メソッドも `&self` とする。

### 9. エラー変換は既存の `wt_err` パターンを踏襲し、0077 で統合する

`serialize_exporter_context` が返す `WtError::invalid_input` は、現行の `wt_err` 関数 (`webtransport.rs` L1053-L1055) により `Error::InvalidArgument` に変換する。本 issue マージ後は `wt_err` 呼び出しが 1 箇所増えるため、0077 (`Error::WebTransport(WtError)` 導入) 実装時の置換対象数として注意する。0077 が 0065 より先にマージされた場合、実装時点で既存パターンに合わせてエラー変換を行うこと。

### 10. rustls の export 失敗は `Error::Tls` にラップする

`rustls::Error` (`HandshakeNotComplete` 等) は I/O ではないため、既存の `Error::Tls(Box<dyn std::error::Error + Send + Sync>)` (`error.rs` L14) が意味論的に正確。`rustls::Error` は `std::error::Error + Send + Sync + 'static` を実装するため `Box<dyn std::error::Error + Send + Sync>` への変換は可能 (rustls 0.23 で確認済み)。TLS ハンドシェイク未完了等で `rustls::Error` が返った場合も `Error::Tls(Box::new(e))` でラップして返す。

加えて、`rustls::ServerConnection::export_keying_material` は失敗時に `output` バッファを呼び出し側に返さない設計 (rustls 0.23 公式ドキュメント: "Ownership of the buffer is taken by the function and returned via the Ok result to ensure no key material leaks if the function fails")。これは部分的に派生した鍵素材が漏れることを防ぐためであり、本実装も `vec![0u8; length]` を closure に渡したあとは Ok/Err どちらでも closure 内で破棄される設計を踏襲し、バッファ再利用やプールは導入しない。`ack.send(res)` が失敗 (受信側 drop) した場合も鍵素材は driver スコープで drop されるため leak しない。

### 11. 後方互換

本変更は `WtServerSession` / `WtSessionHandle` / Sans I/O 層への新規メソッド追加、非公開 `DriverCmd` への新規バリアント追加、新規モジュール公開のみであり、既存公開 API の後方互換を壊さない。

### 12. 単一接続上の単一セッション前提

draft Section 5.3 L686-L688 は「underlying HTTP/2 connection could be shared by multiple WebTransport sessions」と述べているが、現状 tokio-http2 はマルチセッション API (1 接続上で複数の Extended CONNECT を並列駆動する API) を実装していない (1 driver = 1 session)。本 issue もシングルセッション前提で記述する。マルチセッション対応は別 issue で扱う。

## 完了条件

- `src/webtransport/exporter.rs` (新規) に `pub fn serialize_exporter_context(session_id: u64, app_label: &[u8], app_context: &[u8]) -> Result<Vec<u8>, WtError>` が追加されていること
- `src/webtransport/mod.rs` に `pub mod exporter;` と `pub use exporter::serialize_exporter_context;` が追加され、`shiguredo_http2::webtransport::serialize_exporter_context` で参照可能なこと
- `serialize_exporter_context` が以下を満たすこと:
  - `session_id` を 8 バイト big-endian で書き込む
  - `app_label.len() > 255` または `app_context.len() > 255` で `WtError::invalid_input` を返す
  - 長さフィールド (label / context 各 1 バイト) を正しい値で書き込む
- `DriverCmd::ExportKeyingMaterial { app_label: Vec<u8>, app_context: Vec<u8>, length: usize, ack: oneshot::Sender<Result<Vec<u8>>> }` が追加されていること (`Result` は `crate::error::Result` = tokio-http2 層の `Error`、`WtResult` ではない)
- `DriverState::handle_cmd` が `ExportKeyingMaterial` を処理し、以下を満たすこと:
  - `length == 0` または `length > 65535` を `Error::InvalidArgument` で弾く
  - `serialize_exporter_context` を呼び、`ServerConnection::with_tls(|tls| tls.export_keying_material(...))` で TLS exporter を実行する
- `WtServerSession::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` が追加されていること
- `WtSessionHandle::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` が追加されていること
- `tests/test_webtransport/exporter.rs` (新規) に Sans I/O 単体テストが追加されていること (項目は「解決方法 4. テスト戦略」を参照)
- `tests/test_webtransport/main.rs` の挿入順序: `mod exporter;` を `mod capsule;` の直後に追加する。0068 (`mod error;` 追加) がマージ済みであれば `mod capsule;` → `mod error;` → `mod exporter;` → `mod flow_control;` の順序になるよう調整する (0068 とのマージ順序は依存関係セクションを参照)
- `crates/tokio-http2/tests/test_webtransport.rs` に統合テストが追加されていること (項目は「解決方法 4. テスト戦略」を参照)。既存 `test_tls()` helper を再利用する (0063 で TLS 1.3 強制済みのためバージョン再確認は不要)
- `examples/wt_server` が `cargo build --release` で warning なしビルドでき、既存の echo 機能が変更前後で動作することを smoke test で確認すること
- CHANGES.md `## develop` に以下のような `[ADD]` エントリを追加すること:
  ```markdown
  - [ADD] `tokio-http2` に `WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material`、Sans I/O 層に `serialize_exporter_context` を追加する
    - @voluntas
  ```
- `examples/wt_server` に `export_keying_material` の使用例を追加すること。追加位置は `handle_connection` 内で `session.accept(...)` 成功後、`session.into_parts()` 呼び出しの直前とする。得た鍵素材はその長さのみをログ出力し、鍵素材そのもの (バイト列内容) はネットワーク応答やログに出力しない:
  ```rust
  let key_material = session
      .export_keying_material(b"wt-server-example", b"", 32)
      .await?;
  log::debug!(
      "[{remote}] exported keying material: {} bytes",
      key_material.len()
  );
  // セキュリティ注意: 本サンプルでは長さのみログ出力するが、実プロダクションでは zeroize 等で安全に破棄すること。
  // Vec<u8> の Drop はメモリを返すだけで内容を 0 で上書きしないため、鍵素材が swap や core dump に残るリスクがある。
  drop(key_material);
  ```

## 解決方法

### 1. Sans I/O 層: `serialize_exporter_context`

`src/webtransport/exporter.rs` (新規):

```rust
//! WebTransport over HTTP/2 の TLS Keying Material Exporter 用 Exporter Context シリアライズ。
//!
//! draft-ietf-webtrans-http2-14 Section 5.3 で定義される `WebTransport Exporter Context`
//! をバイト列に変換する。本関数の出力は TLS 1.3 exporter API (`rustls::ServerConnection::export_keying_material`)
//! の `context_value` 引数に渡し、固定ラベル `"EXPORTER-WebTransport"` と組み合わせて
//! セッション固有の TLS exporter を導出する。

use crate::webtransport::error::WtError;

/// WebTransport Exporter Context (draft-ietf-webtrans-http2-14 Section 5.3) をシリアライズする。
///
/// `app_label` と `app_context` はいずれも空スライスを許容する。
///
/// # Errors
///
/// `app_label` または `app_context` の長さが 255 バイトを超える場合、`WtError::invalid_input` を返す。
pub fn serialize_exporter_context(
    session_id: u64,
    app_label: &[u8],
    app_context: &[u8],
) -> Result<Vec<u8>, WtError> {
    if app_label.len() > 255 {
        return Err(WtError::invalid_input("exporter label exceeds 255 bytes"));
    }
    if app_context.len() > 255 {
        return Err(WtError::invalid_input(
            "exporter context exceeds 255 bytes",
        ));
    }
    // サイズが事前に確定するため 1 回で確保する
    let total = 8 + 1 + app_label.len() + 1 + app_context.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&session_id.to_be_bytes());
    out.push(app_label.len() as u8); // チェック済みのため as u8 は安全
    out.extend_from_slice(app_label);
    out.push(app_context.len() as u8);
    out.extend_from_slice(app_context);
    Ok(out)
}
```

注: ドックコメント内の draft 引用は節番号のみとし、行番号 (`L696-L702` 等) は含めない。行番号は draft 改訂で変動するため、issue 本文側のみが行番号を持つ。

`src/webtransport/mod.rs` に以下を追加:

```rust
pub mod exporter;
pub use exporter::serialize_exporter_context;
```

### 2. tokio-http2 層: DriverCmd と handle_cmd

`crates/tokio-http2/src/webtransport.rs` の `WEBTRANSPORT_PROTOCOL` 定数 (L29 付近) の近くに TLS exporter ラベル定数を追加:

```rust
// draft-ietf-webtrans-http2-14 Section 5.3 で固定された TLS exporter ラベル。
// draft-ietf-webtrans-http3-15 Section 3.4 と共通。
const EXPORTER_LABEL: &[u8] = b"EXPORTER-WebTransport";
```

配置: 本 issue 時点では TLS exporter 呼び出し (= label の使用箇所) が tokio-http2 層のみのため、tokio-http2 層に置く。複数の I/O 層実装が出現したら Sans I/O 層に移動して再エクスポートする。

`DriverCmd` enum (L638-L673) の末尾、`Drain` variant の直後に追加:

```rust
ExportKeyingMaterial {
    app_label: Vec<u8>,
    app_context: Vec<u8>,
    length: usize,
    ack: oneshot::Sender<Result<Vec<u8>>>,
},
```

`crates/tokio-http2/src/webtransport.rs` の既存 use 文に `serialize_exporter_context` を追加:

```rust
use shiguredo_http2::webtransport::{
    serialize_exporter_context, WtConfig, WtEvent, WtInit, WtSession, WtSessionState, WtStreamId,
    stream::stream_id as wt_stream_id,
};
```

`DriverState::handle_cmd` (L726-L836) の `DriverCmd::Drain` arm の直後に追加:

```rust
DriverCmd::ExportKeyingMaterial { app_label, app_context, length, ack } => {
    let res = (|| -> Result<Vec<u8>> {
        if length == 0 || length > 65535 {
            return Err(Error::InvalidArgument(
                "export length must be in 1..=65535".into(),
            ));
        }
        let session_id = u64::from(self.connect_stream_id.as_u32());
        let ctx = serialize_exporter_context(session_id, &app_label, &app_context)
            .map_err(wt_err)?;
        // rustls 0.23 は失敗時に出力バッファを返さない設計のため、ここで割り当てたバッファは
        // 成功時のみ Ok(T) で返される。Err の場合は closure 内で破棄され、部分派生鍵素材を漏らさない。
        let output_buf = vec![0u8; length];
        let output = self
            .conn
            .with_tls(|tls| {
                tls.export_keying_material(output_buf, EXPORTER_LABEL, Some(ctx.as_slice()))
            })
            .map_err(|e| Error::Tls(Box::new(e)))?;
        Ok(output)
    })();
    // 本 arm は TLS exporter を呼ぶだけで wt_session の状態を変更しないため、
    // 他 arm のような flush_wt_output() は不要。with_tls は同期 API なので closure 内で
    // await を挟まず、output_buf の move は driver タスクの async 境界を超えない。
    let _ = ack.send(res);
}
```

clippy 注意点: `let mut output = vec![0u8; length];` のように `mut` を付けない (バッファ自体は move されるので `mut` 不要)。また `output_buf` と `output` を別名にして shadowed_unrelated 警告を回避する。

### 3. 公開 API: WtServerSession / WtSessionHandle

`WtServerSession` impl (L283-L371) と `WtSessionHandle` impl (L406-L455) に同じ doc comment と実装を追加する:

```rust
impl WtServerSession {
    /// セッション固有の TLS Keying Material Exporter を導出する。
    ///
    /// 本メソッドは `&self` で呼び出せる (内部で `cmd_tx.send` のみ行う)。
    ///
    /// 返り値は暗号鍵素材に相当する機密情報であるため、必要に応じて呼び出し側で
    /// `zeroize` 等を使用して安全に破棄すること。
    ///
    /// `app_label` と `app_context` はいずれも空スライスを許容する。
    /// 空スライスを渡すと、WebTransport Exporter Context の該当部分がゼロ長となる。
    pub async fn export_keying_material(
        &self,
        app_label: &[u8],
        app_context: &[u8],
        length: usize,
    ) -> Result<Vec<u8>> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::ExportKeyingMaterial {
                app_label: app_label.to_vec(),
                app_context: app_context.to_vec(),
                length,
                ack,
            })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }
}
```

`WtSessionHandle` 側も `WtServerSession` と同一の doc comment・実装を追加する。

### 4. テスト戦略

`tests/test_webtransport/` (Sans I/O 層単体テスト) と `crates/tokio-http2/tests/test_webtransport.rs` (tokio-http2 層統合テスト) は別ディレクトリであり混同しないこと (0066 にも同等の注意書きあり)。AGENTS.md 規約に従いテストログは日本語で記述する。

**Sans I/O 単体テスト** (`tests/test_webtransport/exporter.rs` 新規):

- 出力サイズ・先頭 8 バイト big-endian・長さフィールド一致
- 空 label と空 context の境界 (`session_id = 1`、`app_label = b""`、`app_context = b""` で `[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]`)
- 空でない label と context が混在する場合のバイト列順序 (`app_label = b"label"`、`app_context = b"context"` で Session ID → label 長 → label → context 長 → context の順)
- 255 バイトちょうどの label / context が成功
- label と context が両方とも 255 バイトのケースが成功
- 256 バイトの label / context で `WtError::invalid_input`
- 1024 バイト等のさらに長い label / context でも `WtError::invalid_input` で fail-fast (Vec 確保失敗にならないこと)
- `session_id` の境界値 `0`、`1`、`0x7FFF_FFFF`、`0x8000_0000`、`0xFFFF_FFFF`、`u64::MAX` で正しく big-endian 8 バイトになること。`0x8000_0000` 以上は HTTP/2 Stream ID として無効だが、シリアライズ関数の挙動確認として検証する
- 異なる `session_id` で異なるバイト列になること (上記境界値テストに含めて差分確認)

**tokio-http2 統合テスト** (`crates/tokio-http2/tests/test_webtransport.rs`、既存 `test_tls()` helper を再利用):

- TLS 1.3 接続上で `export_keying_material(b"label", b"ctx", 32)` が 32 バイト返すこと
- 同一セッション・同一引数で 2 回呼んで同一バイト列が返ること (冪等性)
- 同一セッションで `app_label` を `b"a"` / `b"b"` と変えると異なる鍵素材が返ること
- 同一セッションで `app_context` を `b"x"` / `b"y"` と変えると異なる鍵素材が返ること
- `length == 1` で 1 バイト返ること (rustls 0.23 は HkdfExpand の出力を `out.len()` に切り詰めるため、1 バイトでも動作する)
- `length` が HKDF-Expand 上限 (`255 * Hash.length`) を超える場合、`Error::Tls` が返ること。テストでは `length = 20000` を使用する (TLS 1.3 ciphersuite は rustls 内部で決まり SHA-256 / SHA-384 のどちらが選択されるか不定なため、両 hash の上限 SHA-256: `255 * 32 = 8160`、SHA-384: `255 * 48 = 12240` をいずれも確実に超える値とする)。エラー判定は variant の存在 (`matches!(err, Error::Tls(_))`) のみで行い、`Display` メッセージ部分一致は使わない (rustls 文字列が将来変わるため)。ciphersuite 依存になる中間値 (8160 < length <= 12240) はテストしない
- `length == 0` を `Error::InvalidArgument` で弾くこと
- `length > 65535` を `Error::InvalidArgument` で弾くこと
- `app_label` / `app_context` が 256 バイトで `Error::InvalidArgument` が返ること
- 2 つの独立した TCP/TLS 接続を別個に張り、同じ `app_label` / `app_context` / `length` を与えても TLS マスターシークレットが異なるため異なる鍵素材が返ること
- `session.into_parts()` 前に `WtServerSession::export_keying_material(b"l", b"c", 32)` で取得した値を保持し、その後 `into_parts()` で取得した `parts.handle.export_keying_material(b"l", b"c", 32)` の戻り値が前者と完全一致すること (driver 側で同じ session_id が使われることの検証)。テスト終了時は `parts.driver.abort()` で driver タスクを打ち切る (tokio 1.x の `JoinHandle::abort` はメモリリークしない)
- `session.close(0, "")` 後の `session.export_keying_material(...)` 呼び出しが `Error::ConnectionClosed` を返すこと

注: 統合テストでは `tokio_http2::Client` が内部 TLS コネクションをカプセル化しており、クライアント側の `export_keying_material` を呼び出す公開 API がないため、サーバー側の出力長・冪等性・label / context 感度のみを検証する。`session_id` の context 組み込み検証は Sans I/O 単体テストで行う。

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 2 (Protocol Overview), L204-L210 — Session ID = CONNECT stream ID の定義
- draft-ietf-webtrans-http2-14 Section 5.3 (Use of Keying Material Exporters), L683-L708
- draft-ietf-webtrans-http2-14 Section 7 (Security Considerations), L1425-L1439 — TLS 1.3 または TLS 1.2 + EMS の要件
- RFC 9113 Section 5.1.1 (Stream Identifiers), L904-L907 — 31-bit Stream ID の根拠
- RFC 9113 L273 — HTTP/2 フィールドのネットワークバイト順 (big-endian) 定義
- RFC 9113 L278-L281 — HTTP/2 が RFC 9000 Section 1.3 の表記規則を採用していることの定義
- RFC 9000 Section 1.3 (`refs/rfc9000.txt` L446-L454) — 構文表記の慣用
- RFC 8446 Section 7.5 (Exporters) — TLS 1.3 の TLS-Exporter 関数定義
- RFC 8446 Section 7.1 — HKDF-Expand-Label の `uint16 length` フィールド
- RFC 5869 — HKDF-Expand の出力上限 (`255 * Hash.length`)

注: `refs/rfc8446.txt` および `refs/rfc5869.txt` は未収録。本 issue では現状節で必要な文面を引用済みのため、実装に追加の原典確認は不要。RFC 8446 / RFC 5869 の収録は `update-refs` 対象として別途扱う。

## 依存関係

- 実装済み前提: 0063 (TLS バージョン要件チェック) — `ServerConnection::with_tls` は 0063 で導入済み (`crates/tokio-http2/src/server.rs` L178-L189)
- 実装済み前提: 0064 (WebTransport-Init ヘッダー) — `WtServerRequest::accept()` 内の処理順序は 0064 で確定済み
- 実装順序: 0063 → 0064 → 0065
- 関連: 0066 (`WtServerRequest::accept()` のシグネ変更と `test_webtransport.rs` の修正) — 現状の `accept()` は 2 引数 (`config`, `allowed_origin`) になっている。統合テストおよび `examples/wt_server` は現状のシグネチャを使用すること
- 関連: 0068 (`tests/test_webtransport/main.rs` に `mod error;` 追加) — 0068 と本 issue は同じ `tests/test_webtransport/main.rs` を編集するため、マージ順序によりコンフリクト解決が必要。最終順序は `mod capsule;` → `mod error;` (0068) → `mod exporter;` (本 issue) → `mod flow_control;` → 残り。可能であれば 0068 を先にマージしてから本 issue を実装する
- 関連: 0074 (draft 参照更新) — 0074 マージ後、issue 本文およびソースコメント内の `draft-ietf-webtrans-http2-14 Section X.Y L###-L###` という行番号付き引用は古くなる。実装着手時には最新版該当節の行番号を確認すること。ソースコメント側には行番号を含めず節番号のみ書く方針 (解決方法 1. 注記) のため、ソース側の更新は不要。可能であれば 0074 を先にマージしてから本 issue を実装する
- 関連: 0077 (`Error::WebTransport(WtError)` 導入) — 詳細は設計判断 9 を参照
