# shiguredo_nghttp2::Session の user_data ポインタ管理を見直す

- Priority: Medium
- Created: 2026-06-12
- Polished: {Polished}
- Model: Opus 4.7
- Branch: feature/refactor-shiguredo-nghttp2-session-pointer-management

## 目的

`shiguredo_nghttp2::Session` の `nghttp2_session_set_user_data` 経由の `self` ポインタ管理を見直し、move 後の dangling 問題を構造的に排除する。`set_user_data` メソッドの可視性も `pub` から `pub(crate)` 等に絞り、利用者が誤って呼べないようにする。

issue 0069 (`bug-fix-nghttp2-send-set-user-data`、`Session::send()` での `set_user_data()` 呼び忘れ修正) のスコープ外として明示的に分離された作業。0069 は `recv()` と対称に `send()` の冒頭で `set_user_data()` を呼ぶ最小修正に留め、根本的な設計見直しは本 issue で扱う。

## 優先度根拠

- 現状の設計は「`Session` を move しない / `recv`・`send` の冒頭で必ず再登録する」という暗黙の不変条件に依存しており、将来 `submit_*` 系で callback を発火させる API を追加するときに同じ呼び忘れバグが再発するリスクがある
- `set_user_data` が `pub` のため外部から誤って呼ばれる可能性があり、意図しないアドレスが登録される潜在的バグの温床
- `nghttp2_session_set_user_data` の自前管理を完全に排除できれば、`recv`/`send` の冒頭の重複呼び出しも不要になり、API の単純化に繋がる
- `shiguredo_nghttp2` クレートは未リリースのため、`Session` 構造の breaking change を許容できる窓のうちに対応する

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

- `self as *mut Session as *mut c_void` を nghttp2 に登録するが、`Session` は `Send + Sync` 実装の通常構造体で `Pin`/`Box` で固定されていない。`Session` を move するとアドレスが変わり、登録済みポインタが dangling になる
- 現状は `recv()`/`send()` の冒頭で毎回 `set_user_data()` を呼び直すことで「move 後に再登録される」設計だが、これは「callback を発火させ得る API すべての先頭で再登録する」前提に依存している
- `submit_request` / `submit_data` 等の `submit_*` 系は現状 callback を発火させないため `set_user_data` を呼んでいないが、将来 `nghttp2_session_resume_data` が callback を発火する経路に変わったり、新しい submit API が追加されたりした場合、同じ呼び忘れバグが再発する
- `set_user_data` が `pub` のため、外部から `session.set_user_data()` を誤って呼べる。これは内部実装の詳細であり、`pub(crate)` 以下に絞るべき
- 加えて、`submit_request(headers, None, true)` 後の `submit_data` の挙動 (data provider が未登録のため `nghttp2_session_resume_data` が失敗する可能性) が API ドキュメントに明示されていない

## 設計方針

### 案 A: `Pin<Box<Session>>` 化

`Session::client()` / `Session::server()` の戻り値を `Pin<Box<Session>>` に変更し、構造体のアドレスが固定されることを型レベルで保証する。`set_user_data` は `new()` 内で 1 度だけ呼び、`recv()`/`send()` の冒頭の重複呼び出しを削除する。

メリット: 型レベルで move を禁止できる。`set_user_data` を private 化しやすい。
デメリット: 公開 API シグネチャが変わる (利用者影響あり)。`PhantomPinned` の管理が必要。

### 案 B: `Arc<UnsafeCell<Session>>` 化

`Session` を `Arc<UnsafeCell<...>>` で包み、ヒープ上にアロケートしてアドレスを固定する。

メリット: move 後の dangling 問題を完全に排除。
デメリット: API が大きく変わる。`Send`/`Sync` の手動実装が必要。

### 案 C: 自前管理ではなく nghttp2 の callback ごとに状態を渡す設計

`set_user_data` を使わず、callback ごとに必要な情報を引数として渡す設計に変更する。

メリット: グローバル状態への依存を排除。
デメリット: nghttp2 の callback API がこの方式をサポートしていない可能性が高い (要調査)。

### 推奨方針

案 A (`Pin<Box<Session>>`) を採用するのが現実的。利用者影響は `Session::client()` / `Session::server()` の戻り値型変更のみに留まり、`tokio-nghttp2` の `Connection` も `Pin<Box<Session>>` を保持する形に書き換えるだけで対応可能。

詳細な設計は `/polish-issue` で磨き上げる際に確定する。

## 完了条件

- `Session::set_user_data` が `pub(crate)` 以下の可視性に絞られている、もしくは設計変更により外部公開メソッドから消えている
- `Session` のアドレスが固定されることが型レベルで保証されている
- `Session::recv()` / `Session::send()` の冒頭の `set_user_data()` 重複呼び出しが削除されている (型レベル保証により不要になるため)
- `submit_request(headers, None, true)` 後の `submit_data` 呼び出しの挙動 (data provider 未登録時の `nghttp2_session_resume_data` の失敗) が `submit_data` の doc コメントに明示されている
- 既存テスト (`crates/shiguredo_nghttp2/tests/` および `crates/tokio-nghttp2/tests/`) が退行しない
- `tokio-nghttp2::Connection` の保持方式が新しい `Session` 型に追従している
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリが追加されている
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 解決方法

issue 0069 マージ後に着手する。設計方針 (案 A 推奨) の詳細を `/polish-issue` で磨き上げてから実装する。

## 参照

- `issues/closed/0069-bug-fix-nghttp2-send-set-user-data.md` — 先行 issue (`Session::send()` での `set_user_data` 呼び忘れ修正)。本 issue のスコープ外として分離された経緯が書かれている
- `crates/shiguredo_nghttp2/src/session.rs` — `Session` 型定義、`set_user_data` メソッド、`recv` / `send` の現状実装
- `crates/tokio-nghttp2/src/connection.rs` — `Session` を保持する上流。設計変更時に追従が必要
- `crates/tokio-nghttp2/src/client.rs` — `Client::connect()` 等の経路
- nghttp2 公式ドキュメント (`nghttp2_session_set_user_data` / `nghttp2_session_resume_data` の挙動)
