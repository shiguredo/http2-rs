# ACK 済み自広告 SETTINGS を参照する API を追加する

- Created: 2026-11-09
- Completed: {YYYY-MM-DD}
- Branch: feature/add-acked-settings-api
- Polished: 2026-11-09

## 目的

draft-ietf-webtrans-http2-16 Section 4.3.1 は、セッション開始の時点でピアから ACK を受けた自広告 SETTINGS 値がセッションの初期値になることを規定する (refs/draft-ietf-webtrans-http2-16.txt Section 4.3.1 の「most recently acknowledged immediately before the CONNECT request is sent」)。まだ ACK されていない SETTINGS は、ACK された後の新しいセッションにしか適用されない。現状 `Connection` は自広告値全体 (`local_settings`) と受信したピア値 (`remote_settings`) を参照できるが、自広告値のうちピア ACK 済みの値を参照する API がない。そこで、自広告 SETTINGS のうちピア ACK 済みの値を参照する `Connection::acked_settings()` を追加し、tokio ドライバの WebTransport accept で仕様どおりの値を使う。

## 現状

- `src/connection.rs` の `Connection` は `local_settings()` (自広告値。`Connection::new` 以降は不変) と `remote_settings()` (受信したピア値。受信時に当該 SETTINGS への ACK を即座にキューへ積むため、参照時点で ACK 送信済み) を参照できるが、自広告値を「ピアが ACK した後の値」で参照する API はない
- 自広告 SETTINGS をネットワークへ送るのは `Connection::initiate()` と `Connection::send_settings()` の 2 箇所だけで、いずれも `local_settings.to_settings_list()` からフレームを組み立てる。`Connection::pending_settings_count` はフレーム数だけ管理し、どの値が ACK されたかは記録していない
- `Connection::handle_settings` は SETTINGS ACK を受信すると `pending_settings_count` を減らし、未送信 SETTINGS への ACK は RFC 9113 Section 6.5.3 の同期が取れないため PROTOCOL_ERROR 接続エラーとする (実装判断)
- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept` は `WtConfig::apply_advertised_settings(conn.local_settings())` で自広告値をそのまま採用する。コメントは「ACK 済みの自広告 SETTINGS 初期値を適用する」とするが、ACK 状態の確認はしていない
- HTTP/2 は単一の順序付きストリームであるため、「クライアントが CONNECT を送るより前にサーバー SETTINGS に応答した ACK フレーム」は必ず CONNECT フレームより先に届き、処理される。したがって、accept 時点までに処理した SETTINGS ACK (CONNECT フレームの処理より先に処理した ACK) に対応する自広告値は、draft-ietf-webtrans-http2-16 Section 4.3.1 の「CONNECT 送信直前に ACK 済みの SETTINGS」と一致する。逆に、accept 時点までにピアの SETTINGS ACK を 1 つも処理していない場合は「CONNECT 送信直前に ACK 済みの SETTINGS が存在しない」ことになり、セッションの自広告値は未広告 (Initial Value 0) に倒すのが draft-ietf-webtrans-http2-16 Section 4.3.1 / Section 11.2 上で正しい
- 未送信 SETTINGS への ACK に対する PROTOCOL_ERROR 検証は既存のテストに存在しない

## 設計方針

- `Connection` に自広告 SETTINGS のバッチ管理を追加する
  - 送信した SETTINGS 1 フレーム (= 1 バッチ) の値をスナップショットして FIFO 待ち行列に保持する。スナップショットは `Settings::apply` を `local_settings.to_settings_list()` の要素に初期化した `Settings` に適用して組み立てる (`initiate()` と `send_settings()` でフレーム組み立てを共有する場合は helper 化してよい)
  - `Connection::handle_settings` の SETTINGS ACK 受信時は FIFO の先頭 1 バッチを取り出し、ACK 確定値の集約 (`Settings`) に適用する。未送信 SETTINGS への ACK に対する PROTOCOL_ERROR は FIFO の空判定に置き換えるだけで、挙動は不変
- 公開 getter `Connection::acked_settings(&self) -> &Settings` を追加する。doc コメントには次を書く
  - ピアから ACK を受信した分の自広告 SETTINGS 値の集約であること。ACK をまだ受けた SETTINGS がない場合、この値は未広告 (全項目が既定値) であること。1 つでも ACK を受けた SETTINGS がある場合、自広告値と同じ値であること (自広告 SETTINGS は不変で、バッチ間の値は同一のため)
  - WebTransport 用途では、未広告の WT 初期値は draft-ietf-webtrans-http2-16 Section 4.3.1 / Section 11.2 により Initial Value 0 として扱われること ([`WtConfig::apply_advertised_settings`](src/webtransport.rs) 側の挙動と一致)。なお、draft-ietf-webtrans-http2-16 Section 4.3.1 が定める決定点 (CONNECT 送信直前・セッション受入直前) をこの getter の doc に書くと現在の実装の範囲外になるため、それ自体は tokio 側のコメントから参照する
- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept` を `WtConfig::apply_advertised_settings(conn.acked_settings())` に変え、draft-ietf-webtrans-http2-16 Section 4.3.1 に基づいて「CONNECT フレームの処理までに ACK を処理した自広告 SETTINGS」という根拠をコメントに書く
- この変更は正常経路上の wire に影響しない。通常のハンドシェイクではクライアントはサーバー SETTINGS に対して CONNECT の前に ACK を返すため、accept 時点の集約は自広告値と一致し、accept が使う値は従来どおりである。この変更で挙動が変わるのは、accept 時点で自広告 SETTINGS への ACK をまだ 1 つも処理していない場合だけで、その場合は Initial Value 0 に倒すことが draft-ietf-webtrans-http2-16 Section 4.3.1 上で正しい挙動となる
- テスト
  - `tests/test_connection.rs` にサン I/O 単体テストを追加する。クライアント / サーバー対で次を確認する: 最初の SETTINGS ACK 受信までは `acked_settings()` が未広告値を返すこと (WT 初期値等の未広告判定に寄与する部分のみを対象とする)、最初の SETTINGS ACK 受信後は `acked_settings()` と `local_settings()` が同値であること (`Settings` の `PartialEq` で getter どうしを直接比較する)、`send_settings()` を連続 2 回呼び出しても、ACK の受信回数が変化するまで外部から観測可能な値が変わらないこと (バッチ間の値は同一のため)
  - 未送信 SETTINGS への ACK は既存どおり PROTOCOL_ERROR で接続エラーになる挙動を検証するテストを新規に追加する (現状この検証はどこにも存在しないため)
  - `pbt/tests/prop_connection/settings.rs` には PBT を追加しない。既存の SETTINGS プロパティが SETTINGS ACK 返送とプリフェイスを検証する範囲を踏襲し、バッチ FIFO と確定値の集約は決定的なシナリオ (`tests/test_connection.rs`) で検証する
- `skills/shiguredo-http2/SKILL.md` の Connection API 表に `acked_settings()` を追加する

## スコープ外

- 自広告 SETTINGS 値の途中更新 (`local_settings` は `Connection::new` 以降不変のままである)
- tokio 層への WebTransport クライアント接続側実装の追加 (現行の tokio 層には WebTransport クライアント接続側の API が存在せず、sans-io の `WtSession::client` 利用が前提であるため)。draft-ietf-webtrans-http2-16 Section 4.3.1 の「CONNECT 送信直前の時点で ACK されたサーバー SETTINGS」は接続順序から `remote_settings()` と一致するため、本 issue では触れない
- HTTP/3 を含む他プロトコルへの移植は範囲外とする

## 完了条件

- `Connection::acked_settings()` が公開 API として `src/connection.rs` に追加され、doc コメントが完成している
- サン I/O 単体テストで、未 ACK の状態では未広告値であること、ACK 後の自広告値との一致が検証されている
- SETTINGS ACK の FIFO 待ち行列と確定値集約が `Connection::handle_settings` に含まれ、未送信 SETTINGS への ACK が PROTOCOL_ERROR 接続エラーになる挙動が (既存の実装判断のまま) テストされた挙動として保たれる
- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept` が `acked_settings()` を使い、draft-ietf-webtrans-http2-16 Section 4.3.1 の根拠コメントが付いている
- WT accept 及び既存 WT フローの非回帰: 通常のハンドシェイクで ACK が CONNECT の前に処理されるため、既存の WT 初期値の挙動が不変であることを既存テスト (crates/tokio-http2/tests/test_webtransport.rs) で確認する
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る
- `skills/shiguredo-http2/SKILL.md` の Connection API 表に `acked_settings()` が追記されている

## 参照

- `src/connection.rs` — `Connection` / `Connection::initiate` / `Connection::send_settings` / `Connection::handle_settings` / `Connection::pending_settings_count` / `Connection::local_settings` / `Connection::remote_settings`
- `src/settings.rs` — `Settings` / `Settings::apply` / `Settings::to_settings_list`
- `src/webtransport.rs` — `WtConfig::apply_advertised_settings` / `WtConfig::overlay_settings`
- `crates/tokio-http2/src/webtransport.rs` — `WtServerRequest::accept`
- `tests/test_connection.rs` / `pbt/tests/prop_connection/settings.rs` — SETTINGS 関連の既存テスト
- `crates/tokio-http2/tests/test_webtransport.rs` — WT ハンドシェイクの既存テスト
- `refs/draft-ietf-webtrans-http2-16.txt` — Section 4.3 / Section 4.3.1 / Section 11.2
- `refs/rfc9113.txt` — Section 6.5.3 (Settings Synchronization)
