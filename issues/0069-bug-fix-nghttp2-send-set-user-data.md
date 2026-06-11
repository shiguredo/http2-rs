# shiguredo_nghttp2::Session::send() が set_user_data() を呼ばない問題を修正する

- Priority: High
- Created: 2026-06-11
- Polished: 2026-06-11
- Model: deepseek-v4-pro
- Branch: feature/fix-nghttp2-send-set-user-data

## 目的

`shiguredo_nghttp2::Session::send()` (`crates/shiguredo_nghttp2/src/session.rs` の `send()` メソッド) が `set_user_data()` を呼ばない。`recv()` (同ファイルの `recv()` メソッド) は冒頭で `self.set_user_data()` を呼んでおり、非対称になっている。

`Session` は `Send + Sync` 実装の通常構造体で、`Box`/`Pin` で固定されていない。さらに `Session::new()` は `set_user_data()` を呼ばないため、`Session::client()` / `Session::server()` 直後の `user_data` は NULL のまま。設計上、callback を発火させ得る `recv()` / `send()` の冒頭で **毎回** `self.set_user_data()` を呼び直し、現在の `self` アドレスを `nghttp2_session_set_user_data` で再登録する不変条件になっている (move されてアドレスが変わったときも追従できる)。`send()` がこの不変条件から外れているのが本バグ。

結果として、`recv()` を経由せず `send()` を先に呼ぶ経路で次の問題が起きる:

1. data provider read callback (`data_source_read_callback`) で `get_session()` が `None` を返し、`NGHTTP2_ERR_CALLBACK_FAILURE` (-902, fatal) を返してセッションが壊れる
2. データを伴わない送信経路でも、`on_frame_send_callback` 等の諸 callback が `get_session()` の None 分岐で握り潰され、`Http2Event::FrameSent` 等のイベント発行や `last_error_message` 取得が欠落する (fatal ではないが観測不能になる)

## 優先度根拠

- 1. の `NGHTTP2_ERR_CALLBACK_FAILURE` は fatal であり、`nghttp2_session_mem_send` が負値を返してセッション全体が破棄される。後続の通信は不可能
- 発生条件は「TLS 接続直後の最初のリクエストが DATA 付き (POST / PUT) で、上位の event loop に入る前に送信を完了する」ような実装で容易に成立する。`tokio-nghttp2::Client::connect()` 経由の利用でも経路として踏まれ得る
- 2. のイベント取りこぼしは黙って発生するため、ユーザー側から発見しにくい (`recv` 経由でセッションを温めた後に挙動を検証するテストでは再現しない)
- 修正コストは極小 (1 行追加) で回帰リスクも低い (`recv()` で同等の呼び出しが既に稼働中)

## 現状の問題

`crates/shiguredo_nghttp2/src/session.rs` の `recv()` メソッド冒頭は正しく `self.set_user_data()` を呼んでいる:

```rust
pub fn recv(&mut self, data: &[u8]) -> Result<usize> {
    self.set_user_data();
    let result = unsafe {
        nghttp2_sys::nghttp2_session_mem_recv(self.session, data.as_ptr(), data.len())
    };
    check_nghttp2_with_value(result as i32).map(|v| v as usize)
}
```

一方 `send()` は呼んでいない:

```rust
pub fn send(&mut self) -> Result<Vec<u8>> {
    self.output.clear();

    loop {
        let mut data_ptr: *const u8 = ptr::null();
        let len = unsafe { nghttp2_sys::nghttp2_session_mem_send(self.session, &mut data_ptr) };
        // ...
    }

    Ok(std::mem::take(&mut self.output))
}
```

### 再現条件

- `Session::client()` / `Session::server()` 直後は `user_data` 未設定
- そのまま `recv()` を 1 度も呼ばずに送信側を駆動すると `send()` 内の `nghttp2_session_mem_send` が `user_data` が NULL のまま各 callback を発火させる

read callback が走り fatal となる経路:
- `Session::submit_request(headers, Some(data), end_stream=true)` → `send()`: `build_data_provider2()` で `data_source_read_callback` が登録されており、DATA 送信時に発火 → `get_session()` で None → CALLBACK_FAILURE
- `Session::submit_request(headers, None, end_stream=false)` → `submit_data(stream_id, data, true)` → `send()`: 同様
- `Session::submit_response(stream_id, headers, end_stream=false)` → `submit_data(stream_id, data, true)` → `send()`: サーバー側で同等

callback は走るが fatal ではない経路 (`submit_settings` 等) の具体例は直後の「tokio-nghttp2 経由での影響」セクションで詳述する。

### tokio-nghttp2 経由での影響

`tokio-nghttp2::Client::connect()` は接続直後に `conn.submit_settings(&[]).await?` を呼ぶ。`Connection::submit_settings()` は `session.submit_settings(...) → flush() → session.send()` を呼び出し、この時点で `recv()` は 1 度も呼ばれていない。

- このときは SETTINGS フレームのみで `data_source_read_callback` は発火しないため fatal は出ない
- しかし `on_frame_send_callback` が user_data null で走り、`Http2Event::FrameSent { stream_id: 0, frame_type: Settings }` がイベントキューに入らないまま握り潰される
- その後 `Client::send_request(headers, Some(body), end_stream)` を呼ぶと `submit_request → flush → send` の経路で `data_source_read_callback` が user_data null で走り fatal を返す

本 issue の修正のみで `tokio-nghttp2` 経由の経路もカバーされる。`tokio-nghttp2` 側のコード変更は不要。

## 設計方針

- `recv()` と対称に、`send()` の冒頭で `self.set_user_data()` を呼ぶ
- 「`Session::new()` で 1 度だけ呼ぶ」案は採らない。`Session` を `Pin<Box<Self>>` 等で固定していないため、move 後にアドレスが変わるとポインタが dangling になる。本 issue は既存の「呼び出しごとに再登録する」設計を `send()` にも徹底させる最小修正に留める
- 修正後コードのコメントは `recv()` と同等の粒度 (= コメントなし、または 1 行のみ) に揃え、両者の対称性を保つ

## スコープ外

- `Session` の self への生ポインタを user_data として登録するアプローチ自体の見直し (`Pin<Box<Session>>` 化 / `Arc<UnsafeCell<Session>>` 化 / `set_user_data()` の `pub(crate)` 化など) は別 issue で扱う。本 issue は既存設計を維持して `send()` を `recv()` と対称に揃える最小修正
- `submit_request` / `submit_response` / `submit_data` などの `submit_*` 系自体の先頭への `set_user_data()` 追加は本 issue では行わない。理由: `submit_*` は内部キューに積むだけで callback を発火させない (`submit_data` の `nghttp2_session_resume_data` も含む)。callback 発火は `mem_recv` / `mem_send` 内に閉じており、これら 2 つの先頭で再登録すれば必要十分
- `submit_request(headers, None, true)` 後に `submit_data` を呼んだ場合の `nghttp2_session_resume_data` の挙動 (data provider 未登録のため失敗する可能性) の API ドキュメント整備は本 issue では行わない
- `error_callback2` 経由の `last_error_message` の検証はテスト難度が高い (`send()` 側で `error_callback2` を意図的に発火させるのが困難) ため、本 issue のテストでは `data_source_read_callback` と `on_frame_send_callback` の 2 経路に絞る
- 0071 (`refactor-remove-send-error`) は `shiguredo_http2` クレートの `SendError` 型を扱う別作業で、本 issue (`shiguredo_nghttp2::Session::send()`) とは無関係。順序依存なし

## 対応手順

1. 作業ブランチ `feature/fix-nghttp2-send-set-user-data` を作成する
2. `crates/shiguredo_nghttp2/src/session.rs` の `Session::send()` メソッド冒頭 (`self.output.clear();` の直前) に `self.set_user_data();` を 1 行追加する。`recv()` と同様にコメントは付けない (対称性を保つ)
3. `crates/shiguredo_nghttp2/tests/test_session.rs` を新規作成する。本 issue 着手時点で `crates/shiguredo_nghttp2/tests/` ディレクトリは存在せず、これが初の追加。`shiguredo-rust` 規約の「単体テストのファイル名は `tests/test_<module>.rs`」に従う。Cargo は `tests/*.rs` を自動で integration test として認識するため `Cargo.toml` の `[[test]]` エントリ追加は不要
4. 上記テストファイルに以下のテストを追加する。テストは `Session` の公開 API (`submit_*` / `send` / `poll_event` / `last_error_message`) のみを用い、private フィールドへのアクセスや `#[cfg(test)] pub(crate)` の追加は行わない。失敗時メッセージは `expect("理由")` で日本語明示する。テスト関数の doc コメントは日本語で書く:
   - `test_send_before_recv_with_data_provider_succeeds`: クライアント側で DATA 付き submit_request を経由する経路を検証する。既存 `src/lib.rs::tests::test_session_send` は SETTINGS のみで `data_source_read_callback` が発火しないため修正前でも `Ok` を返すが、本テストは DATA を載せて `data_source_read_callback` を走らせる点で性質が異なる
     - `Session::client()` → POST 用ヘッダー (`Header::method("POST")`, `Header::scheme("https")`, `Header::authority("example.com")`, `Header::path("/")`) で `submit_request(&headers, Some(b"hello"), true)` → `send()` を呼ぶ
     - `submit_request` の戻り値 stream_id > 0 を assert
     - 修正前: `send()` 内の `data_source_read_callback` が user_data null で `CALLBACK_FAILURE` を返し、`send()` が `Err(_)` を返す
     - 修正後: `send()` が `Ok(output)` を返し、`poll_event()` で得られるイベント列に `matches!(event, Http2Event::FrameSent { frame_type: FrameType::Data, .. })` を満たすものが少なくとも 1 件存在する
     - `Err` の variant 種別までは assert しない (`result.is_err()` のみで足りる。具体的な variant は本 issue のスコープ外)
   - `test_send_with_submit_data_succeeds`: `submit_request(headers, None, false)` + `submit_data` 経由の経路を検証する (上のテストと同じ `data_source_read_callback` を踏むが、別経路でも修正が効くことを追加保証)
     - `Session::client()` → POST 用ヘッダーで `submit_request(&headers, None, false)` → 戻り値 stream_id を取得 → `submit_data(stream_id, b"hello", true)` → `send()`
     - 同上の修正前後の挙動を assert
   - `test_send_settings_emits_frame_sent_event`: 既存 `src/lib.rs::tests::test_session_send` は送信バイト列の有無のみ確認するが、本テストは FrameSent イベントの有無を確認する点で目的が異なる。本 issue の (2) の問題 (callback 握り潰しによるイベント欠落) の主力回帰テスト
     - `Session::client()` → `submit_settings(&[])` → `send()` を呼ぶ。`send()` は修正前後どちらも `Ok` を返す
     - 修正前: `poll_event()` で得られるイベント列に `matches!(event, Http2Event::FrameSent { stream_id: 0, frame_type: FrameType::Settings })` を満たすものが見つからない (callback が user_data null で握り潰されイベントが入らない)
     - 修正後: 同パターンマッチを満たすイベントが少なくとも 1 件存在する
5. 追加したテストが回帰テストとして機能していることを動作確認する。具体的には、本 Step 2 で追加した `self.set_user_data();` 行を一時的にコメントアウトして `cargo test -p shiguredo_nghttp2 --test test_session` を実行し、追加したテストが少なくとも 1 件失敗することを確認する。確認後、コメントアウトを元に戻して `cargo test` を再実行し、全件通過することを確認する
6. `CHANGES.md` の `## develop` セクション内の既存 `[FIX]` 群の末尾に以下のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネストする。CHANGES.md は変更概要のみを記し、詳細な再現条件は本 issue / commit message に残す:

   ```markdown
   - [FIX] `shiguredo_nghttp2::Session::send()` の冒頭で `set_user_data()` を呼ぶように修正し、`recv()` を経由せずに `send()` を呼ぶ経路でも各コールバックが正しい `Session` ポインタを受け取れるようにする (issue 0069)
     - @voluntas
   ```

7. `cargo fmt --all -- --check` で整形違反がないことを確認する
8. `cargo test --workspace` で全テスト通過を確認する (新規追加した再現テストが通ること、既存の `src/lib.rs::tests::test_session_send` 等が退行しないこと)
9. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する。clippy 警告が出た場合は `#[allow(...)]` で抑制せず、コード自体を修正する

## 完了条件

- `Session::send()` の冒頭に `self.set_user_data();` 呼び出しが追加されている (`self.output.clear();` の直前)
- 修正後、`Session::recv()` と `Session::send()` の冒頭で同じ `self.set_user_data()` を呼ぶ対称性が保たれている
- `recv()` を経由せずに `send()` を直接呼ぶシナリオを 3 件カバーする integration test が `crates/shiguredo_nghttp2/tests/test_session.rs` に追加されている (DATA 付き送信 2 経路 + FrameSent イベント取得)
- 各テストが修正前のコードでは少なくとも 1 件失敗し、修正後は全件通過することを動作確認している (回帰テストとしての有効性確認)
- `CHANGES.md` の `## develop` に `[FIX]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する

## 解決方法

### `Session::send()` の修正 (`crates/shiguredo_nghttp2/src/session.rs`)

`recv()` と同じパターンで、関数冒頭に `self.set_user_data()` を 1 行追加する (コメント不要、`recv()` と対称):

```rust
pub fn send(&mut self) -> Result<Vec<u8>> {
    self.set_user_data();
    self.output.clear();

    loop {
        let mut data_ptr: *const u8 = ptr::null();
        let len = unsafe { nghttp2_sys::nghttp2_session_mem_send(self.session, &mut data_ptr) };
        // 以降は既存のまま
    }

    Ok(std::mem::take(&mut self.output))
}
```

### テスト雛形 (`crates/shiguredo_nghttp2/tests/test_session.rs`)

新規作成するテストファイルの冒頭と 1 件目の雛形。残り 2 件も同形で書く。

```rust
use shiguredo_nghttp2::{FrameType, Header, Http2Event, Session};

/// recv() を経由せず DATA 付き submit_request → send() を呼んでも
/// data_source_read_callback で NGHTTP2_ERR_CALLBACK_FAILURE にならないこと
#[test]
fn test_send_before_recv_with_data_provider_succeeds() {
    let mut session = Session::client().expect("クライアントセッションが生成できること");
    let headers = vec![
        Header::method("POST"),
        Header::scheme("https"),
        Header::authority("example.com"),
        Header::path("/"),
    ];
    let stream_id = session
        .submit_request(&headers, Some(b"hello"), true)
        .expect("submit_request が成功すること");
    assert!(stream_id > 0, "クライアント開始ストリーム ID は正の値");

    // 修正前は send() 内の data_source_read_callback が user_data null で
    // CALLBACK_FAILURE を返し send() が Err になる。
    let output = session.send().expect("send が CALLBACK_FAILURE を返さないこと");
    assert!(!output.is_empty(), "送信バイト列が空でないこと");

    // FrameSent イベントが正しく流れることを追加で確認する
    let mut saw_data_frame = false;
    while let Some(event) = session.poll_event() {
        if matches!(
            event,
            Http2Event::FrameSent {
                frame_type: FrameType::Data,
                ..
            }
        ) {
            saw_data_frame = true;
        }
    }
    assert!(saw_data_frame, "DATA フレームの FrameSent イベントが取得できること");
}
```

## 参照

- `crates/shiguredo_nghttp2/src/session.rs` の `Session::set_user_data()` — `self as *mut Session as *mut c_void` を `nghttp2_session_set_user_data` に登録する実装
- `crates/shiguredo_nghttp2/src/session.rs` の `get_session()` 関数 — user_data が null の場合 None を返す。null でない場合は無条件に `&mut *(user_data as *mut Session)` を返すため、登録時のアドレスが解放済み / move 後のアドレスに変わっている場合は UB となる (move ごとの再登録が必要な根拠)
- `crates/shiguredo_nghttp2/src/session.rs` の `data_source_read_callback` — `get_session()` が None のとき `NGHTTP2_ERR_CALLBACK_FAILURE` を返す経路
- `crates/shiguredo_nghttp2/src/session.rs` の `on_frame_send_callback` — `get_session()` が None のときイベントを push しない経路 ((2) の問題の発生箇所)
- `crates/tokio-nghttp2/src/client.rs` の `Client::connect()` / `crates/tokio-nghttp2/src/connection.rs` の `Connection::submit_settings()` / `Connection::flush()` — `recv()` を経由せずに `Session::send()` を呼ぶ経路の上流
