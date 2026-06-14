# shiguredo_nghttp2::Session の user_data ポインタ管理を見直す

- Priority: High
- Created: 2026-06-12
- Polished: 2026-06-14
- Model: Opus 4.7
- Branch: feature/refactor-shiguredo-nghttp2-session-pointer-management

## 目的

`shiguredo_nghttp2::Session` の `nghttp2_session_set_user_data` 経由の `self` ポインタ管理を見直し、move 後の dangling 問題を構造的に排除する。`set_user_data` メソッドは削除し、`SessionData` の構築時に 1 度だけ登録する。

issue 0069 (`bug-fix-nghttp2-send-set-user-data`、`Session::send()` での `set_user_data()` 呼び忘れ修正) のスコープ外として明示的に分離された作業。0069 は `recv()` と対称に `send()` の冒頭で `set_user_data()` を呼ぶ最小修正に留め、根本的な設計見直しは本 issue で扱う。

## 優先度根拠

- 現状の設計は「`Session` を move しない / `recv`・`send` の冒頭で必ず再登録する」という暗黙の不変条件に依存しており、将来 `submit_*` 系で callback を発火させる API を追加するときに同じ呼び忘れバグが再発するリスクがある
- `set_user_data` が `pub` のため外部から誤って呼ばれる可能性があり、意図しないアドレスが登録される潜在的バグの温床
- `self as *mut Session` を FFI callback 経由で再解釈する設計であり、move 後 dangling pointer のリスクはメモリ安全性に関わる。単なる API 整理ではなく unsafe 境界の不変条件を型で固定する作業なので Priority は High とする
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

- `self as *mut Session as *mut c_void` を nghttp2 に登録するが、`Session` は `Send + Sync` 実装の通常構造体で `Pin<Box<Session>>` 等で固定されていない。`Session` を move するとアドレスが変わり、登録済みポインタが dangling になる
- 0069 適用後の暫定設計は `recv()`/`send()` の冒頭で毎回 `set_user_data()` を呼び直すことで「move 後に再登録される」形になるが、これは「callback を発火させ得る API すべての先頭で再登録する」前提に依存しており、新しい API 追加時に同じ呼び忘れバグが再発するリスクがある
- `submit_request` / `submit_data` 等の `submit_*` 系は現状 callback を発火させないため `set_user_data` を呼んでいないが、将来 `nghttp2_session_resume_data` が callback を発火する経路に変わったり、新しい submit API が追加されたりした場合、同じ呼び忘れバグが再発する
- `set_user_data` が `pub` のため、外部から `session.set_user_data()` を誤って呼べる。これは内部実装の詳細であり、`pub(crate)` 以下に絞るべき

## 設計方針

### 採用方針: `Session` ラッパー + `Pin<Box<SessionData>>`

`Session` のフィールドを持つ内部構造体 `SessionData` を新設し、`Session` は `Pin<Box<SessionData>>` を所有するラッパーとする。外部向け API は既存の `Session` のまま維持する。

```rust
use std::marker::PhantomPinned;
use std::pin::Pin;

/// 内部セッションデータ
///
/// `PhantomPinned` により `!Unpin` とし、ヒープ上でアドレスが固定される。
pub(crate) struct SessionData {
    session: *mut nghttp2_sys::nghttp2_session,
    role: SessionRole,
    events: VecDeque<Http2Event>,
    output: Vec<u8>,
    pending_headers: HashMap<StreamId, Vec<Header>>,
    pending_data: HashMap<StreamId, Vec<u8>>,
    send_buffers: HashMap<StreamId, StreamSendBuffer>,
    last_error_message: Option<String>,
    _pin: PhantomPinned,
}

/// nghttp2 セッション
pub struct Session {
    inner: Pin<Box<SessionData>>,
}
```

- `SessionData` は `PhantomPinned` を含むため `!Unpin` となり、`Pin<Box<SessionData>>` 経由でのアドレス固定が型レベルで保証される
- `Session` ラッパー自体は通常の構造体なので、利用者が `Session::client()` の戻り値を move しても `SessionData` のヒープアドレスは変わらない
- コールバックに渡す `user_data` は `SessionData` へのポインタとし、`Session::new()` 内で 1 度だけ `nghttp2_session_set_user_data` を呼ぶ
- `recv()` / `send()` の冒頭の `set_user_data()` 呼び出しは削除する
- `Session::set_user_data()` メソッドは削除する (外部に公開しない)
- コールバック関数は `*mut c_void` から `*mut SessionData` にキャストし直す
- `Session` ラッパーは `Send` / `Sync` を自動導出する。`SessionData` は raw pointer を含むため `unsafe impl Send / Sync` が必要。既存 `Session` の SAFETY コメントを `SessionData` に移す
- `Drop` は `SessionData` に実装し、`nghttp2_session_del` でセッションを解放する。`Session` ラッパーには `Drop` を実装しない

### なぜ `Pin<Box<Session>>` ではなくラッパー + inner か

- `Pin<Box<Session>>` にすると `Session` の全メソッドが `self: Pin<&mut Self>` を要求し、`tokio-nghttp2::Connection` 等の呼び出し側も `Pin` 操作が必要になり API 影響が大きい
- ラッパー + inner の形なら `Session` のメソッドは既存の `&mut self` のまま維持でき、呼び出し側の変更がほぼ不要
- `SessionData` は `pub(crate)` とし、利用者が直接触ることはない

## 対応手順

1. 作業ブランチ `feature/refactor-shiguredo-nghttp2-session-pointer-management` を作成する
2. `crates/shiguredo_nghttp2/src/session.rs` に `SessionData` 構造体を新設する:
   - 既存 `Session` の全フィールドを移す
   - `PhantomPinned` フィールドを追加して `!Unpin` にする
   - `unsafe impl Send for SessionData {}` / `unsafe impl Sync for SessionData {}` を追加し、既存 `Session` の SAFETY コメントを移す
3. `Session` を `inner: Pin<Box<SessionData>>` のみを持つラッパーに変更する
4. `Session::new()` 内で `SessionData` を構築し、`Box::pin` した直後に `nghttp2_session_set_user_data` を 1 度だけ呼び出す。`user_data` には `SessionData` へのポインタを登録する:

   ```rust
   let mut inner = Box::pin(SessionData::new(...)?);
   unsafe {
       // SAFETY: `inner` はこれから `Session` ラッパーに束縛される。
       // この時点ではまだ利用者に到達しておらず `Pin` 契約を破る操作はないため、
       // `get_unchecked_mut()` で `&mut SessionData` を取得しても安全である。
       nghttp2_sys::nghttp2_session_set_user_data(
           inner.as_ref().get_ref().session,
           inner.as_mut().get_unchecked_mut() as *mut SessionData as *mut c_void,
       );
   }
   ```
5. `Session::set_user_data()` メソッドを削除する
6. `Session::recv()` / `Session::send()` の冒頭から `self.set_user_data()` を削除する
7. `&self` を取るメソッド (`role`, `want_write`, `want_read`, `get_remote_settings`, `get_local_settings`, `get_outbound_queue_size`, `get_next_stream_id`, `get_last_proc_stream_id`, `last_error_message`, `get_remote_window_size`, `get_local_window_size`, `get_stream_remote_window_size`, `get_stream_local_window_size`) では、`self.inner.as_ref().get_ref()` で `&SessionData` を取得する
8. `&mut self` を取るメソッド (`recv`, `send`, `poll_event`, `submit_request`, `submit_response`, `submit_data`, `submit_data_for_trailer`, `submit_rst_stream` 等) では、`unsafe` ブロック内で `self.inner.as_mut().get_unchecked_mut()` を呼び、`&mut SessionData` を取得して実行する
9. `SessionData` に `Drop` を実装し、`nghttp2_session_del` でセッションを解放する (元の `Session` の `Drop` を移動)。`Session` ラッパーには `Drop` を実装しない
10. コールバックから呼ばれる private メソッド (`push_event`, `push_header`, `push_data`, `take_headers`, `take_data` 等) を `SessionData` に移動する
11. FFI コールバック (`on_frame_recv_callback`, `on_data_chunk_recv_callback` 等) で `*mut c_void` から `*mut SessionData` にキャストし直す。`get_session` ヘルパーの戻り値を `Option<&'a mut SessionData>` に変更する
12. `crates/shiguredo_nghttp2/tests/test_session.rs` と `crates/shiguredo_nghttp2/src/lib.rs` 内のテストを確認する。`Session::client()` / `Session::server()` の戻り値が `Session` ラッパーのままなので原則変更不要だが、`Pin` 操作が必要になった場合は追従する
13. `crates/tokio-nghttp2/src/connection.rs` を確認する。`Connection` は `Session` ラッパーをそのまま所有するため原則変更不要だが、メソッド呼び出しで `Pin` 操作が必要になった場合は追従する
14. `CHANGES.md` の `## develop` セクションの先頭に以下のエントリと担当者行を追加する (`shiguredo-issues` 規約により issue 番号は含めない):

    ```markdown
    - [CHANGE] `shiguredo_nghttp2::Session` の `nghttp2_session_set_user_data` ポインタ管理を見直し、`SessionData` を `Pin<Box>` で固定して move 後の dangling を構造的に排除する
      - @voluntas
    ```

15. `cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過することを確認する

## 完了条件

- `crates/shiguredo_nghttp2/src/session.rs` に `SessionData` 構造体が定義され、`PhantomPinned` により `!Unpin` になっている
- `Session` が `Pin<Box<SessionData>>` を所有するラッパーになっており、利用者に対する API は既存のまま維持されている
- `Session::set_user_data()` メソッドが削除されている
- `Session::recv()` / `Session::send()` の冒頭の `set_user_data()` 呼び出しが削除されている
- `SessionData` へのポインタが `Session::new()` 内で 1 度だけ `nghttp2_session_set_user_data` に登録されている
- FFI コールバックが `*mut SessionData` を正しく復元できるようになっている
- `get_session` ヘルパー関数が `Option<&'a mut SessionData>` を返すようになっている
- コールバックから呼ばれる private メソッド (`push_event` 等) が `SessionData` に移動している
- `&self` メソッドが `self.inner.as_ref().get_ref()` 経由で `&SessionData` にアクセスしている
- `SessionData` に `Drop` が実装され、`Session` ラッパーには `Drop` が実装されていない
- `SessionData` の `unsafe impl Send` / `unsafe impl Sync` に SAFETY コメントが移されている
- 既存テスト (`crates/shiguredo_nghttp2/tests/` および `crates/tokio-nghttp2/tests/`) が退行しない
- `tokio-nghttp2::Connection` が新しい `Session` ラッパーをそのまま利用できている (必要に応じて追従済み)
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリが追加されている (issue 番号なし)
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 解決方法

issue 0069 マージ後に着手する。`crates/shiguredo_nghttp2/src/session.rs` の `Session` をラッパー + `Pin<Box<SessionData>>` 構成に変更し、上記対応手順に従って実装する。

## 参照

- `issues/closed/0069-bug-fix-nghttp2-send-set-user-data.md` — 先行 issue (`Session::send()` での `set_user_data` 呼び忘れ修正)。本 issue のスコープ外として分離された経緯が書かれている
- `shiguredo-issues` スキル — issue 番号を含めてはいけない場所 (CHANGES.md) の規約
- `shiguredo-changelog` スキル — `[CHANGE]` エントリの扱い
- `shiguredo-rust` スキル — Rust コーディング規約
- `crates/shiguredo_nghttp2/src/session.rs` — `Session` 型定義、`set_user_data` メソッド、`recv` / `send` の現状実装
- `crates/tokio-nghttp2/src/connection.rs` — `Session` を保持する上流。設計変更時に追従が必要
- `crates/tokio-nghttp2/src/client.rs` — `Client::connect()` 等の経路
- nghttp2 公式ドキュメント (`nghttp2_session_set_user_data` の挙動)
