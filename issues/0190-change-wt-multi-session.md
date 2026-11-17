# 1 コネクション 1 セッション固定を解消し、複数 WebTransport セッションを扱えるようにする

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-wt-multi-session
- Polished: 2026-11-17

## 目的

draft-ietf-webtrans-http2-16 Section 4.1 は、HTTP/2 の SETTINGS_MAX_CONCURRENT_STREAMS が同一 HTTP/2 接続上の WebTransport セッション数を制限することを定めている。Section 5.1 は pooling をサポートし、「Every WebTransport session is an independent HTTP/2 stream and does not compete with other pooled WebTransport sessions for per-stream resources.」としている。現行実装は `WtServerRequest` が `ServerConnection` を所有し、`accept()` が接続を丸ごと driver に渡すため、1 接続 1 セッション固定になっている。accept 後に届く 2 本目の CONNECT や通常リクエストは driver に無視され、クライアントは応答を得られずタイムアウトする。接続レベルの dispatcher を追加して複数セッションを扱えるようにし、既存の単一セッション API の制約は公開 doc とプロトコル応答で明示する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::from_connection` は `ServerConnection` を受け取り、`accept()` は `DriverState` に接続をムーブする。`DriverState::handle_event` は CONNECT ストリーム以外のイベントを `_ => {}` で捨てる。`WtServerRequest` から `ServerConnection` を取り戻す API はない。

公開 doc には「1 接続 1 セッション」という制約が書かれていない。モジュール doc は「`WtServerRequest::accept()` 時に単一の driver タスクを spawn し」と述べるだけで、接続を所有することには触れていない。`crates/tokio-http2/README.md` の `WtServerRequest` の説明も「`accept()` で受諾、`reject(status)` で拒否」までで、接続が消費されることを明示していない。`examples/wt_server/src/main.rs` の `handle_connection` は accept 前に非 WebTransport リクエストを受けると接続を閉じる。

## 設計方針

- 接続レベルの dispatcher を新設する。dispatcher は `ServerConnection` を所有して `next_event()` を回し、CONNECT ストリーム ID ごとに WebTransport driver タスクを spawn する。driver は dispatcher へのコマンドチャネルを持ち、`send_response` / `send_data` / `reset_stream` / `send_window_update` / TLS exporter などの接続操作は dispatcher 経由で行う。dispatcher は受信イベントを CONNECT ストリーム ID で該当 driver に振り分ける。`ServerConnection` を `Arc<Mutex<_>>` で共有せず、単一所有者 + チャネルで実装する。
- 接続レベルの dispatcher 型の名前・シグネチャは実装時に決定する。dispatcher が返す要求の `accept` / `reject` は `ServerConnection` を消費せず、dispatcher が接続を維持する。1 つのセッションが終了しても接続と他のセッションは維持する。driver 終了時のセッション登録解除は `issues/0191-bug-fix-wt-session-parts-lifecycle.md` のライフサイクルと整合させる。
- 既存の `WtServerRequest::from_connection` は単一セッション専用 API として残す。この API は接続を所有するため、accept 後は同一接続で 1 セッションのみを扱う。2 本目以降の CONNECT と通常リクエストは無視せず、`RST_STREAM` (`ErrorCode::RefusedStream`) で明示的に拒否する。RFC 9113 Section 8.7 は REFUSED_STREAM を「アプリケーション処理前に拒否した」ことを示すと定め、draft-ietf-webtrans-http2-16 Section 4.1 もセッション数制限の手段として REFUSED_STREAM を挙げている。接続全体を終了する必要がある場合のみ `GOAWAY` を使う。
- 接続レベルの dispatcher では、複数の CONNECT を受理して複数セッションを確立する。通常リクエストは `RST_STREAM` (`ErrorCode::RefusedStream`) で拒否し、既存セッションと接続は維持する。接続全体を終了する必要がある場合のみ `GOAWAY` を使う。
- 単一セッション API と接続レベルの dispatcher の両方について、公開 doc を更新する。対象は `crates/tokio-http2/src/webtransport.rs` のモジュール doc、`WtServerRequest::from_connection` と `accept` / `reject` の rustdoc、`WtServerSession` の rustdoc、`crates/tokio-http2/README.md`、`README.md` の WebTransport 節、`skills/shiguredo-http2/SKILL.md` の tokio-http2 節とする。単一セッション API には 1 接続 1 セッションの制約と `REFUSED_STREAM` による拒否を明記し、接続レベルの dispatcher には複数セッションを扱えることと、セッション終了が接続全体の終了ではないことを明記する。
- 非 CONNECT ストリームの DATA に対する接続受信ウィンドウの補充は `issues/0186-bug-fix-wt-connection-window.md` の範囲とする。dispatcher を追加する場合も、dispatcher が受信した DATA の接続ウィンドウを補充する (0186 の実装と整合させる)。
- 2 本の WebTransport セッションを同一 HTTP/2 接続で確立し、それぞれでデータを送受信できることを検証する統合テストを追加する。通常リクエストを同じ接続に送り、`REFUSED_STREAM` で拒否され、既存セッションが継続することを検証する。単一セッション API についても、2 本目の CONNECT と通常リクエストが `REFUSED_STREAM` で拒否され、最初のセッションが継続することを検証する。
- 新しい公開 API は `CODEBASE.md` に従い、使用箇所 (`examples/wt_server` またはテスト) とテストを用意する。`examples/wt_server` は接続レベルの dispatcher を使う形に更新する。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しない。
- 接続共有型 API の設計が実装前に確定できない場合のみ `issues/pending/` へ移動する。本 issue では接続レベルの dispatcher を追加する方針を先に確定するため、原則として pending にしない。

## 完了条件

- 同一 HTTP/2 接続で複数の WebTransport セッションを確立できること。
- 既存の単一セッション API は、1 接続 1 セッションの制約が rustdoc と `crates/tokio-http2/README.md` に明記され、2 本目以降の CONNECT と通常リクエストが `RST_STREAM` (`ErrorCode::RefusedStream`) で拒否されること。
- 接続レベルの dispatcher で通常リクエストが `RST_STREAM` (`ErrorCode::RefusedStream`) で拒否され、既存セッションと接続が維持されること。
- 2 本の WebTransport セッションを同一 HTTP/2 接続で確立し、それぞれでデータを送受信できる統合テストが追加されていること。
- 2 本目の CONNECT / 通常リクエストの扱いを検証するテストが追加されていること (接続レベルの dispatcher と単一セッション API の両方)。
- 新しい公開 API が `CODEBASE.md` に従い、使用箇所とテストを持つこと。
- `crates/tokio-http2/src/webtransport.rs` のモジュール doc、`WtServerRequest::from_connection` と `accept` / `reject` の rustdoc、`WtServerSession` の rustdoc、`crates/tokio-http2/README.md`、`README.md` の WebTransport 節、`skills/shiguredo-http2/SKILL.md` の tokio-http2 節が実装と一致していること。
- `CHANGES.md` が変更されていないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。

## 他 issue との関係

- `issues/0186-bug-fix-wt-connection-window.md` — 非 CONNECT ストリームの接続ウィンドウ補充。本 issue の dispatcher / 単一セッション API の変更後も、0186 の補充規則を維持する。
- `issues/0120-add-wt-event-user-notification.md` — `DriverState` のイベント通知。同じファイルを触るためリベースを見込む。
- `issues/0191-bug-fix-wt-session-parts-lifecycle.md` — `DriverState` のライフサイクル。dispatcher のセッション登録解除と整合させる。
- `issues/0198-doc-update-wt-docs.md` — WT 関連の公開 doc の更新。本 issue で追加する dispatcher の doc と重複しないよう、0198 の範囲を確認する。

## 参照

- `refs/draft-ietf-webtrans-http2-16.txt` — Section 4.1 (Limiting the Number of Simultaneous Sessions) / Section 5.1 (Transport Properties, Pooling)
- `refs/rfc9113.txt` — Section 7 (Error Codes, REFUSED_STREAM) / Section 8.7 (Request Reliability)
- `crates/tokio-http2/src/webtransport.rs` — `WtServerRequest` / `DriverState` / `DriverState::handle_event`
- `crates/tokio-http2/src/server.rs` — `ServerConnection`
- `crates/tokio-http2/tests/test_webtransport.rs` — WebTransport サーバー統合テスト
- `crates/tokio-http2/README.md` / `README.md` — WebTransport の公開 doc
- `skills/shiguredo-http2/SKILL.md` — tokio-http2 の公開 doc
- `examples/wt_server/src/main.rs` — 接続レベルの dispatcher を使う形に更新する
