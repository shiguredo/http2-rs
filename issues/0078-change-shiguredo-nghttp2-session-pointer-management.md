# shiguredo_nghttp2::Session の user_data ポインタ管理を見直す

- Priority: High
- Created: 2026-06-12
- Polished: 2026-08-08
- Model: Opus 4.7
- Branch: feature/change-shiguredo-nghttp2-session-pointer-management

## 目的

`shiguredo_nghttp2::Session` の `nghttp2_session_set_user_data` 経由の `self` ポインタ管理を見直し、move 後の dangling 問題を通常の利用経路で実質的に解消する。`set_user_data` メソッドの可視性も `pub` から private に変更し、利用者が誤って呼べないようにする。

issue 0069 (`bug-fix-nghttp2-send-set-user-data`、`Session::send()` での `set_user_data()` 呼び忘れ修正) のスコープ外として明示的に分離された作業。0069 は `recv()` と対称に `send()` の冒頭で `set_user_data()` を呼ぶ最小修正に留め、根本的な設計見直しは本 issue で扱う。

## 優先度根拠

- 現状の設計は「`Session` を move しない / `recv`・`send` の冒頭で必ず再登録する」という暗黙の不変条件に依存しており、将来 `submit_*` 系で callback を発火させる API を追加するときに同じ呼び忘れバグが再発するリスクがある
- `set_user_data` が `pub` のため、内部実装の詳細が外部に公開されている (呼び出し自体は常に自身のアドレスを登録するだけだが、「呼ばなければならない」という不変条件の誤解を生む)
- `self as *mut Session` を FFI callback 経由で再解釈する設計であり、move 後 dangling pointer のリスクはメモリ安全性に関わる。単なる API 整理ではなく unsafe 境界の不変条件を API のシグネチャで表現する作業なので Priority は High とする
- `nghttp2_session_set_user_data` の呼び出しをコンストラクタ内の 1 回に集約できれば、`recv`/`send` の冒頭の重複呼び出しも不要になり、API の単純化に繋がる
- `shiguredo_nghttp2` クレートは canary.0 から GitHub タグで公開済みであり、`Session::client()` 等のシグネチャ変更は破壊的変更になる。正式リリース前のこのタイミングで対応する

## 現状の問題

`crates/shiguredo_nghttp2/src/session.rs` の `Session::set_user_data()`:

```rust
pub fn set_user_data(&mut self) {
    unsafe {
        nghttp2_sys::nghttp2_session_set_user_data(
            self.session,
            self as *mut Session as *mut c_void,
        );
    }
}
```

問題点:

- `self as *mut Session as *mut c_void` を nghttp2 に登録するが、`Session` は `unsafe impl Send` のみの通常構造体で `Pin`/`Box` で固定されていない。`Session` を move するとアドレスが変わり、登録済みポインタが dangling になる
- 0069 適用後の暫定設計は `recv()`/`send()` の冒頭で毎回 `set_user_data()` を呼び直すことで「move 後に再登録される」形になるが、これは「callback を発火させ得る API すべての先頭で再登録する」前提に依存している
- 現状の develop では 0069 がマージ済みのため、`recv()` と `send()` の両方が冒頭で `set_user_data()` を呼び直している。0078 はこの暫定設計の根本課題 (move 後 dangling pointer のリスク) を扱う
- `submit_request` / `submit_data` 等の `submit_*` 系は現状 callback を発火させないため `set_user_data` を呼んでいないが、`submit_data` の内部で呼ばれる `nghttp2_session_resume_data` が deferred DATA を outbound queue に戻し、次の `send()` で callback (data source read callback) が発火する経路がある。新しい submit API が追加された場合に同じ呼び忘れバグが再発するリスクがある
- `set_user_data` が `pub` のため、内部実装の詳細が外部に公開されている。これは private にすべき
- 加えて、`submit_request(headers, None, true)` 後の `submit_data` の挙動 (stream が存在しない、または deferred DATA が存在しないため `nghttp2_session_resume_data` が `NGHTTP2_ERR_INVALID_ARGUMENT` で決定的に失敗する) が API ドキュメントに明示されていない

## 設計方針

### 採用: 案 A (`Pin<Box<Session>>` 化)

`Session::client()` / `Session::server()` / `Session::client_with_options()` / `Session::server_with_options()` の戻り値を `Result<Pin<Box<Session>>>` に変更し、構造体がヒープ上にアロケートされることを型のシグネチャで表現する。

設計の詳細:

- `Session` 自体は `Unpin` のままとする (`PhantomPinned` は入れない)。`Pin<Box<Session>>` は Box のポインタがヒープ上の `Session` を指すため、`Pin<Box<Session>>` を move してもヒープ上のアドレスは不変。これにより `set_user_data` に登録した `self as *mut Session` は、通常の利用経路 (`Pin<Box<Session>>` の保持・move・メソッド呼び出し) では dangling にならない。`Session` を `!Unpin` にすると `&mut self` を取る全メソッド (`recv` / `send` / `submit_*` 等) の呼び出しが unsafe 化するため、`Unpin` のままとする
- ただし `Session: Unpin` のため、`Pin<Box<Session>>` は `From` impl / `Pin::into_inner` 経由で `Box<Session>` に戻し、`Session` を値として取り出して move することが safe に可能である (dangling の再発経路が残る)。また `DerefMut` で `&mut Session` を取得できるため、`std::mem::swap` / `std::mem::replace` でヒープ上の値が入れ替わる経路も残る。そのため `Session::client()` 等の doc コメントに「返された `Pin<Box<Session>>` を `Box` に戻して move しないこと、および `&mut Session` を取得して `std::mem::swap` / `std::mem::replace` しないこと (登録済みの user_data ポインタが dangling になる、または別のセッションの値を指すようになる)」を明記する
- `Session::new()` は `Result<Self>` のまま内部メソッドとして維持し、各コンストラクタ内で `Box::pin(Self::new(...)?)` してから `set_user_data()` を 1 度だけ呼ぶ
- `set_user_data` は `pub` から private に変更する (コンストラクタ内でのみ呼ぶため)
- `recv()` / `send()` の冒頭の `set_user_data()` 重複呼び出しを削除する
- `crates/tokio-nghttp2/src/connection.rs` の `Connection` 構造体の `session` フィールドを `Pin<Box<Session>>` に変更する (`Session::client()` 等の戻り値型変更に追従)
- `submit_data` の doc コメントに、`submit_request(headers, None, true)` 後に呼んだ場合の挙動を明記する (詳細は「解決方法」参照)

### 不採用: 案 B (`Arc<UnsafeCell<Session>>` 化)

`Send`/`Sync` の手動実装が必要になり、API の変更範囲が大きい。`Session` は既に `unsafe impl Send` のみで `Sync` を意図的に実装していない (nghttp2 はスレッドセーフでない) ため、`Arc` 化はこの設計と整合しない。

### 不採用: 案 C (callback ごとに状態を渡す)

nghttp2 の callback API はセッション全体に対する `user_data` を 1 つしか持たず、callback ごとに個別の状態を渡す方式をサポートしていない (stream 単位の `nghttp2_session_set_stream_user_data` は存在するが、セッションレベルの callback からは参照できず、本設計の `Session` ポインタとは役割が異なる)。

## 完了条件

- `Session::set_user_data` が `pub` から private に変更されている
- `Session::client()` / `Session::server()` / `Session::client_with_options()` / `Session::server_with_options()` の戻り値が `Result<Pin<Box<Session>>>` に変更され、doc コメントに「返された `Pin<Box<Session>>` を `Box` に戻して move しないこと、および `&mut Session` を取得して `std::mem::swap` / `std::mem::replace` しないこと」が明記されている
- `Session::recv()` / `Session::send()` の冒頭の `set_user_data()` 重複呼び出しが削除されている (コンストラクタ内での 1 回の登録で済むため)
- `submit_data` / `submit_data_for_trailer` の doc コメントに、`submit_request(headers, None, true)` 後に呼んだ場合の挙動 (stream が存在しない、または deferred DATA が存在しないため `nghttp2_session_resume_data` が `NGHTTP2_ERR_INVALID_ARGUMENT` で失敗し、`send_buffers` に追加済みのデータが送信されずに残る。失敗後に同じ `stream_id` で再試行するとデータが二重に追加される) が明示されている
- コンストラクタ内 1 回登録の回帰テストが `tests/test_session.rs` に追加されている (`Pin<Box<Session>>` を別スコープへ move してから DATA 付き送信を行い、`data_source_read_callback` 経由で送信が成功し `FrameSent` イベントが通知されることの検証。SETTINGS のみの送信では callback が user_data を参照しないため検証にならない)
- 既存テスト (`crates/shiguredo_nghttp2/src/lib.rs` の `#[cfg(test)]` テスト / `crates/shiguredo_nghttp2/tests/` / `crates/tokio-nghttp2/tests/`) が退行しない
- `tokio-nghttp2::Connection` の `session` フィールドが `Pin<Box<Session>>` に追従している
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリが追加されている
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 解決方法

### 変更内容 (`crates/shiguredo_nghttp2/src/session.rs`)

1. `Session::new()` は `Result<Self>` のまま維持する
2. 各コンストラクタ (`client` / `server` / `client_with_options` / `server_with_options`) の戻り値を `Result<Pin<Box<Session>>>` に変更し、`Box::pin(Self::new(...)?)` してから `set_user_data()` を呼ぶ形にする:

   ```rust
   pub fn client() -> Result<Pin<Box<Session>>> {
       let mut session = Box::pin(Self::new(SessionRole::Client, None)?);
       session.set_user_data();
       Ok(session)
   }
   ```

3. `set_user_data` を `pub` から private (`fn set_user_data(&mut self)`) に変更する
4. `recv()` / `send()` の冒頭の `self.set_user_data()` を削除する
5. 各コンストラクタ (`client` / `server` / `client_with_options` / `server_with_options`) の doc コメントに「返された `Pin<Box<Session>>` を `Box` に戻して move しないこと、および `&mut Session` を取得して `std::mem::swap` / `std::mem::replace` しないこと (登録済みの user_data ポインタが dangling になる、または別のセッションの値を指すようになる)」を明記する
6. `submit_data` / `submit_data_for_trailer` の doc コメントに、`submit_request(headers, None, true)` 後に呼んだ場合の挙動を明記する: `nghttp2_session_resume_data` は stream が存在しない、または deferred DATA が存在しない場合に `NGHTTP2_ERR_INVALID_ARGUMENT` で決定的に失敗する。`submit_data` は `send_buffers` にデータを追加した後に resume_data を呼ぶため、失敗時は追加済みのデータが送信されずにキューに残る。`send_buffers` のエントリはピアがストリームをクローズしたとき (`on_stream_close_callback`) にのみ削除されるため、ストリームがクローズされない、またはクローズ済みの `stream_id` を指定した場合は残留し続ける (メモリリークの可能性)。さらに、失敗後に同じ `stream_id` で再試行すると既存データに追加で `extend` され、後に送信されるデータが二重になる

### 追従変更 (`crates/tokio-nghttp2/src/connection.rs`)

`Connection` 構造体の `session` フィールドを `Session` から `Pin<Box<Session>>` に変更する。`Session::client()` 等の戻り値型変更に追従するだけで、メソッド呼び出し (`self.session.recv(...)` 等) は `Pin<Box<Session>>` の `DerefMut` 経由でそのまま動作する。

### CHANGES.md

`## develop` セクションの既存 `[CHANGE]` 群の先頭 (リポジトリの慣習どおり新しいエントリを上に置く) に以下のエントリを追加する:

```markdown
- [CHANGE] `shiguredo_nghttp2::Session` のコンストラクタ戻り値を `Result<Pin<Box<Session>>>` に変更し、`nghttp2_session_set_user_data` への登録をコンストラクタ内の 1 回に集約する。`set_user_data` を private 化し、`recv` / `send` の冒頭の重複呼び出しを削除する (move 後 dangling ポインタのリスク低減)
  - @voluntas
```

## 他 issue との関係

- 0068 (`bug-fix-wt-error-design`) / 0070 (`change-privatize-error-wt-error-fields`) / 0071 (`change-remove-send-error`) / 0072 (`change-remove-unused-code`) / 0073 / 0076 / 0102 / 0103: `CHANGES.md` を編集するため、マージ順序によってはコンフリクトの可能性がある (内容は異なる箇所なので 3-way merge で解決できる見込み)
- 0076 (`fmt-translate-english-comments`): `crates/shiguredo_nghttp2/src/lib.rs` を編集するが、本 issue が編集する `session.rs` とはファイルが異なるため衝突しない。ただし lib.rs の `test_session_send` には `Session::client()` 呼び出しと 0076 の対象 `println!` が同居しており、本 issue が同関数を編集した場合、hunk の近接により 3-way merge でもコンフリクトが発生し得る
- 0102 / 0103: ルートクレート `shiguredo_http2` の `src/connection.rs` が対象で、本 issue が触る `tokio-nghttp2` の `connection.rs` とは別クレート。衝突しない

注記: CHANGES.md への `[CHANGE]` エントリ追加位置は、リポジトリの実績 (新しいエントリを上に置く) に従い「既存 `[CHANGE]` 群の先頭」とする。0071 / 0072 の issue 本文には「末尾に追加」と書かれているが、これはリポジトリ実績と矛盾する誤りであり、実装時は先頭挿入を優先する。

## 対応手順

1. 作業ブランチ `feature/change-shiguredo-nghttp2-session-pointer-management` を作成する
2. `crates/shiguredo_nghttp2/src/session.rs` の `Session::client()` / `Session::server()` / `client_with_options()` / `server_with_options()` の戻り値を `Result<Pin<Box<Session>>>` に変更し、`Box::pin(Self::new(...)?)` してから `set_user_data()` を呼ぶ形にする
3. `set_user_data` を `pub` から private に変更する
4. `recv()` / `send()` の冒頭の `self.set_user_data()` を削除する
5. 各コンストラクタの doc コメントに「`Pin<Box<Session>>` を `Box` に戻して move しないこと、および `&mut Session` を取得して `std::mem::swap` / `std::mem::replace` しないこと」を明記する
6. `submit_data` / `submit_data_for_trailer` の doc コメントに `nghttp2_session_resume_data` の失敗挙動と `send_buffers` の残留副作用を明記する
7. `crates/tokio-nghttp2/src/connection.rs` の `Connection` 構造体の `session` フィールドを `Pin<Box<Session>>` に変更する
8. `tests/test_session.rs` にコンストラクタ内 1 回登録の回帰テストを追加する (`Pin<Box<Session>>` を別スコープへ move してから DATA 付き送信を行い、`data_source_read_callback` 経由で送信が成功し `FrameSent` イベントが通知されることの検証。SETTINGS のみの送信では callback が user_data を参照しないため検証にならない)
9. 回帰テストの有効性を確認するため、コンストラクタ内の `set_user_data()` を一時的にコメントアウトして新規テスト (および既存テスト) が失敗することを確認し、元に戻す
10. 既存テスト (`crates/shiguredo_nghttp2/src/lib.rs` の `#[cfg(test)]` テスト / `crates/shiguredo_nghttp2/tests/` / `crates/tokio-nghttp2/tests/`) が追従することを確認する (コンストラクタの戻り値型変更によるコンパイルエラーがあれば修正)
11. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の先頭に「解決方法」で示した `[CHANGE]` エントリと担当者行を追加する
12. `cargo fmt --all -- --check` で整形違反がないことを確認する
13. `cargo test --workspace` で全テスト通過を確認する
14. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する

## 参照

- `issues/closed/0069-bug-fix-nghttp2-send-set-user-data.md` — 先行 issue (`Session::send()` での `set_user_data` 呼び忘れ修正)。本 issue のスコープ外として分離された経緯が書かれている
- `issues/0071-change-remove-send-error.md` — 公開 API 変更の `[CHANGE]` エントリ追加の同種事例
- `crates/shiguredo_nghttp2/src/session.rs` — `Session` 型定義、`set_user_data` メソッド、`recv` / `send` の現状実装
- `crates/tokio-nghttp2/src/connection.rs` — `Session` を保持する上流。設計変更時に追従が必要
- `crates/tokio-nghttp2/src/client.rs` — `Client::connect()` 等の経路
- nghttp2 公式ドキュメント (`nghttp2_session_set_user_data` / `nghttp2_session_resume_data` の挙動)
