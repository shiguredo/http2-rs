# WT 関連の SKILL / README / rustdoc を実装に合わせる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/update-wt-docs
- Polished: 2026-11-17

## 目的

WebTransport 関連の公開ドキュメントに、実装と一致しない記述が複数残っている。SKILL の `initiate` / `webtransport(...)` 必須 / PADDING 分類 / TLS 1.3 要件 / WT クライアント例、README の PADDING 分類 / exporter 経路 / peer_config 例、tokio-http2 README の `drain`、interop README のクライアント SETTINGS、rustdoc の `Draining` / `varint::encoded_len` / TLS 1.2 テストコメントの記述を実装と仕様に合わせる。SKILL の version / ビルド方式 / `mod.rs` / `ConnectionState` / Event 表と、`README.md` / `crates/nghttp2-sys/README.md` のビルド方式は open 0118 の範囲とし、本 issue では扱わない。`CHANGES.md` は open 0199 の範囲とし、本 issue では扱わない。

## 現状

- `skills/shiguredo-http2/SKILL.md`:
  - `WtSession::initiate()` を「初期 SETTINGS を送信し `Active` に遷移」とするが、実装は状態を `Active` にするだけ。
  - 「WebTransport には `wt_enabled(true)` + `enable_connect_protocol(true)` + `webtransport(...)` が必要 (二重ゲート)」とするが、`LimitsBuilder` の必須条件は `wt_enabled(true)` と `enable_connect_protocol(true)` のみ。`webtransport(...)` は WT 初期値を一括設定する任意のメソッド。
  - 「対応仕様」表の RFC 9297 の行が `PADDING` を含むが、PADDING は draft-16 Section 6.1 の WebTransport Capsule であり、RFC 9297 は DATAGRAM のみを定義する。
  - 「対応仕様」表の draft-16 の行が「TLS 1.3 要件」とするが、draft-16 Section 7 の要件は TLS 1.3 または TLS 1.2 + EMS。実装は rustls 0.23 が EMS 状態を公開しないため TLS 1.3 のみを許可する。
  - WT クライアント例が peer 用に `WtConfig::default()` を渡している。`WtConfig::peer_default()` から開始する規約と矛盾する。
  - version / ビルド方式 / `src/connection/mod.rs` / `ConnectionState` / Event 表は open 0118 の範囲であり、本 issue では変更しない。
- `README.md`:
  - PADDING を RFC 9297 の Capsule Protocol に分類しているが、PADDING は draft-16 Section 6.1。WebTransport Capsule の節に分類する。
  - TLS Keying Material Exporter を `WtServerRequest::accept()` 経由とするが、実際は `WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material`。
  - WT クライアント例が peer 用に `WtConfig::default()` を渡している。`WtConfig::peer_default()` から開始する規約と矛盾する。
  - ビルド方式 (`cmake`) は open 0118 の範囲であり、本 issue では変更しない。
- `crates/tokio-http2/README.md`: `drain()` を「送信中データの完了を待つ」とするが、実装は WT_DRAIN_SESSION を送信して Draining に遷移するだけで、送信中データの完了は待たない。
- `interop/browser/README.md`: `enable_connect_protocol` / `wt_enabled` を「仕様上サーバーが送る設定であり、クライアントからは送られない」とするが、仕様はサーバー送信を MUST とするだけでクライアント送信を禁止していない (RFC 8441 Section 3 / draft-16 Section 3.1)。
- rustdoc:
  - `WtSessionState::Draining` が「新規ストリーム受付停止」となっているが、実装と draft-16 Section 6.13 は新規ストリーム開設を許可する。
  - `varint::encoded_len` の `# Panics` は実装と一致しない。実装は `u64::MAX` でも 8 を返し、パニックしない。
  - `crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_tls12_rejected` の doc コメントが「TLS 1.2 で WebTransport セッションを要求した場合は malformed として扱い」とするが、draft-16 Section 7 は TLS 1.2 + EMS を許容する。実装は rustls 0.23 が EMS 状態を公開しないため TLS 1.3 のみを許可するポリシーで TLS 1.2 を拒否する。
- `README.md` / `crates/nghttp2-sys/README.md` のビルド方式と `CHANGES.md` は、それぞれ open 0118 / 0199 の範囲であり、本 issue では変更しない。

## 設計方針

- SKILL / README / tokio-http2 README / interop README / rustdoc / テストコメントの WT 関連記述を実装と一次資料に合わせて修正する。
- 変更対象:
  - `skills/shiguredo-http2/SKILL.md`: `WtSession::initiate()` を「状態を `Active` に遷移させるだけ」に修正する。`LimitsBuilder` の説明を「`wt_enabled(true)` と `enable_connect_protocol(true)` が必須。`webtransport(...)` は WT 初期値を一括設定する任意のメソッド」に修正する。RFC 9297 の行から PADDING を外し、draft-16 の WebTransport Capsule に分類する。draft-16 の行の「TLS 1.3 要件」を「TLS 1.3 または TLS 1.2 + EMS (実装は TLS 1.3 のみ)」に修正する。WT クライアント例の peer 引数を `WtConfig::peer_default()` に変更する。
  - `README.md`: PADDING を WebTransport Capsule (draft-16 Section 6.1) に分類する。exporter を `WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material` に修正する。WT クライアント例の peer 引数を `WtConfig::peer_default()` に変更する。
  - `crates/tokio-http2/README.md`: `drain()` を「WT_DRAIN_SESSION を送信して Draining に遷移するだけで、送信中データの完了は待たない」に修正する。
  - `interop/browser/README.md`: `enable_connect_protocol` / `wt_enabled` を「サーバーが送る MUST がある設定だが、クライアント送信を禁止しない。実測の false は、送っていない場合の既定値表示である可能性がある (ログから送信の有無は区別できない)」に修正する。
  - `src/webtransport.rs`: `WtSessionState::Draining` を「ドレイン中でも新規ストリーム開設・データ送信・データグラム送信を許可する」に修正する。
  - `src/webtransport/varint.rs`: `encoded_len` の `# Panics` を削除し、常に 1/2/4/8 を返すことを書く。
  - `crates/tokio-http2/tests/test_webtransport.rs`: `test_wt_tls12_rejected` の doc コメントを「実装ポリシーとして TLS 1.3 のみ許可するため TLS 1.2 を拒否する。RFC 9113 の malformed 処理として RST_STREAM(PROTOCOL_ERROR) を送る」に修正する。
- TLS 1.2 は「TLS 1.3 のみを許可する実装ポリシー」と明記し、仕様上 TLS 1.2 + EMS も許容されることを併記する。「TLS 1.2 で WebTransport セッションを要求した場合は malformed として扱い」という一律表現を削除する。
- open 0118 が扱う SKILL の version / ビルド方式 / `src/connection/mod.rs` / `ConnectionState` / Event 表と、`README.md` / `crates/nghttp2-sys/README.md` のビルド方式には変更を加えない。
- `CHANGES.md` は open 0199 の範囲なので変更しない。
- 公開 API のシグネチャと挙動、テストの挙動は変更しない (ドキュメントとコメントのみの変更)。

## 完了条件

- `skills/shiguredo-http2/SKILL.md` の `WtSession::initiate()` が「状態を `Active` に遷移させるだけ」と書かれていること。
- 同 SKILL の `LimitsBuilder` の説明が `wt_enabled(true)` と `enable_connect_protocol(true)` を必須とし、`webtransport(...)` を任意の WT 初期値設定メソッドとして説明していること。
- 同 SKILL の「対応仕様」表で PADDING が draft-16 Section 6.1 に分類され、RFC 9297 の行から外れていること。
- 同 SKILL の「対応仕様」表の draft-16 の行が TLS 1.3 または TLS 1.2 + EMS を仕様要件とし、実装が TLS 1.3 のみを許可するポリシーであることを区別していること。
- 同 SKILL の WT クライアント例の peer 引数が `WtConfig::peer_default()` であること。
- `README.md` の PADDING が WebTransport Capsule (draft-16 Section 6.1) に分類され、RFC 9297 の Capsule Protocol の節から外れていること。
- `README.md` の exporter が `WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material` と書かれていること。
- `README.md` の WT クライアント例の peer 引数が `WtConfig::peer_default()` であること。
- `crates/tokio-http2/README.md` の `drain()` が「WT_DRAIN_SESSION を送信して Draining に遷移するだけで、送信中データの完了は待たない」と書かれていること。
- `interop/browser/README.md` が `enable_connect_protocol` / `wt_enabled` について「サーバーが送る MUST がある設定だが、クライアント送信を禁止しない」と書いていること。
- `src/webtransport.rs` の `WtSessionState::Draining` が「ドレイン中でも新規ストリーム開設・データ送信・データグラム送信を許可する」と書かれていること。
- `src/webtransport/varint.rs` の `encoded_len` に `# Panics` がなく、常に 1/2/4/8 を返すことが書かれていること。
- `crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_tls12_rejected` の doc コメントが、TLS 1.2 + EMS を許容する仕様要件と TLS 1.3 のみを許可する実装ポリシーを区別し、RFC 9113 の malformed 処理として `RST_STREAM(PROTOCOL_ERROR)` を送ることを書いていること。
- open 0118 の範囲 (SKILL の version / ビルド方式 / `src/connection/mod.rs` / `ConnectionState` / Event 表、`README.md` と `crates/nghttp2-sys/README.md` のビルド方式) に変更が入っていないこと。
- `CHANGES.md` に差分がないこと。
- 公開 API のシグネチャと挙動、テストの挙動が変わっていないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
