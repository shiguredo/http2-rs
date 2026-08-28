# shiguredo_nghttp2 のテストを追加する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/add-shiguredo-nghttp2-tests
- Polished: 2026-08-28

## 目的

`shiguredo_nghttp2` クレートで、本クレートのテストから一度も呼ばれていない公開関数・メソッドにテストを追加し、対象範囲について CODEBASE.md「公開 API は必ずテストで動作を保証すること（テストされていない公開 API を公開しない）」を満たす。対象は `Session` / `validation.rs` / `SessionOptions` / `types.rs` の公開関数・メソッドに限定する (`error.rs` の生成されない `Error` variant と、`Http2Event` の variant は下の「対象外」参照)。ここで「一度も呼ばれていない」は、テストコードから直接の呼び出し箇所が無いことを指す (`FrameType::from_u8` のように本クレートのコード内部から呼ばれ、結果がイベントとして観測されるものも含む)。

テスト追加 (削除ではない) を選ぶ理由は次のとおり。`shiguredo_nghttp2` は nghttp2 C API をラップする低レベル境界であり、`SessionOptions` のビルダー群は `crates/shiguredo_nghttp2/README.md` と `skills/shiguredo-http2/SKILL.md` の双方に、`validation.rs` の検証関数 7 個は `crates/shiguredo_nghttp2/README.md` の「ユーティリティ」に、それぞれ公開 API として記載されている (検証関数は SKILL.md に記載が無いため、記載を根拠に含めない)。コアクレート (`shiguredo_http2`) の公開 API 整理で削除を検討した 0113 / 0072 とは性質が異なる。

## 現状

2026-08-28 時点の実測。テストは合計 15 件で、`cargo test -p shiguredo_nghttp2` は全件通過する。

- `crates/shiguredo_nghttp2/tests/test_session.rs`: 4 件。すべて `Session::client()` の正常系のみ
- `crates/shiguredo_nghttp2/src/lib.rs` の `#[cfg(test)] mod tests`: 11 件。`nghttp2_version` / `Header` / `ErrorCode` / `SettingsId` と、`Session` の構築・`role()`・`submit_settings()`・`send()`・`want_write()` を検証している。サーバー側は `Session::server()` の構築を確認する `test_server_session_new` のみで、サーバー側の送信 API は未検証
- 本クレートのテストから一度も呼ばれていない `Session` の公開メソッド (`Session::client` / `Session::server` / `role` / `send` / `poll_event` / `submit_settings` / `submit_request` / `submit_data` / `want_write` 以外すべて):
  - 送受信基盤: `recv()` (正常系も異常系も未検証)
  - サーバー側送信: `submit_response()` / `submit_headers()` / `submit_trailer()` / `submit_data_for_trailer()`
  - 制御フレーム: `submit_rst_stream()` / `submit_goaway()` / `submit_ping()` / `submit_window_update()` / `submit_shutdown_notice()` / `terminate_session()`
  - 状態参照: `want_read()` / `get_remote_settings()` / `get_local_settings()` / `get_outbound_queue_size()` / `get_next_stream_id()` / `get_last_proc_stream_id()` / `last_error_message()`
  - フロー制御: `get_remote_window_size()` / `get_local_window_size()` / `get_stream_remote_window_size()` / `get_stream_local_window_size()` / `set_local_window_size()` / `consume()` / `consume_connection()` / `consume_stream()`
  - オプション構築: `client_with_options()` / `server_with_options()`
- `crates/shiguredo_nghttp2/src/types.rs` の公開 API のうち、`Header::new` と `FrameType::from_u8` は本クレートのテストから一度も呼ばれていない (`Header::method` / `Header::status` / `Header::sensitive` / `Header::name_str` / `Header::value_str` / `ErrorCode::as_u32` / `ErrorCode::from_u32` / `SettingsId::as_i32` は `src/lib.rs` の `#[cfg(test)]` で検証済み。`Header::scheme` / `Header::authority` / `Header::path` は `tests/test_session.rs` で `submit_request()` の入力として構築されるだけで、返り値の assert は無い)
- `crates/shiguredo_nghttp2/src/validation.rs` の公開 7 関数 (`check_header_name` / `check_header_value_rfc9113` / `check_method` / `check_path` / `check_authority` / `http2_strerror` / `is_fatal`) は `src/lib.rs` の `pub use` で公開されているが、テストからも他のクレートからも一度も呼ばれていない
- `crates/shiguredo_nghttp2/src/options.rs` の `SessionOptions` ビルダーは 10 個のうち `peer_max_concurrent_streams` だけが `crates/tokio-nghttp2/tests/client_server.rs` の `test_session_options` で使われ、残り 9 個 (`no_auto_window_update` / `no_auto_ping_ack` / `max_send_header_block_length` / `max_deflate_dynamic_table_size` / `max_outbound_ack` / `max_settings` / `stream_reset_rate_limit` / `max_continuations` / `glitch_rate_limit`) は呼び出し箇所がゼロ
- 上記のうち、ワークスペース全体 (TLS 経由の `crates/tokio-nghttp2/tests/client_server.rs` と `crates/tokio-http2/tests/interop.rs`) を通しても一度も実行されないのは、`consume` 系 3 関数、フロー制御の getter / setter 5 関数、`submit_window_update()`、`want_read()`、`get_outbound_queue_size()`、`get_next_stream_id()`、`get_last_proc_stream_id()`、`last_error_message()`、`validation.rs` の 7 関数、`SessionOptions` の未使用ビルダー 9 個である。それ以外の `submit_*` 系は `crates/tokio-nghttp2/src/connection.rs` のラッパー経由で統合テストが通るが、エラーパスは見ていない

## 設計方針

### テストの構成

- 追加テストは `crates/shiguredo_nghttp2/tests/` に公開 API 経由で書く (`tests/` は公開 API に対してだけ書くという shiguredo-rust 規約)
- `crates/shiguredo_nghttp2/src/lib.rs` の `#[cfg(test)] mod tests` 11 件の移管は本 issue の対象外とする (`issues/closed/0036-refactor-move-mod-tests-to-tests-dir.md` が `crates/` 配下の `#[cfg(test)]` をスコープ外としている前例に従う)。新規テストは `tests/` 側にのみ追加する
- `tests/test_session.rs` にセッション API のテストを追加し、`tests/test_validation.rs` を新設して `validation.rs` の 7 関数を検証する
- 追加規模が `tests/test_session.rs` に収まらなくなった場合は、`issues/0111-refactor-split-reset-stream-tests.md` が示したとおりに `tests/test_session/main.rs` + サブモジュールへ移行する。`tests/test_session.rs` と `tests/test_session/main.rs` は Cargo のターゲット名が衝突するため同時存在させない

### ピア模擬 (モックは使わない)

- AGENTS.md「モックやスタブは絶対に利用しないこと」に従い、偽のピアは作らない。`Session::client()` と `Session::server()` を両方作り、`send()` の返すバイト列を相手の `recv()` に渡して同期往復させるヘルパーを `tests/test_session.rs` 内に定義する
- **往復には `submit_settings()` の明示呼び出しが必要**。2026-08-28 の実測で、次の 2 通りの挙動を確定した
  - 失敗する構成: `Session::client()` に `submit_request()` させただけ (両側で `submit_settings()` を呼ばない) で 1 回目の `send()` 出力 (実測で 42 バイト。送信するヘッダー集合で変わる) をサーバーに `recv()` させると、全バイトは消費されるがサーバー側に `Http2Event::HeadersReceived` は発生せず `get_last_proc_stream_id()` も 0 のままになる。往復を 6 回繰り返しても同じだった。サーバー側は「SETTINGS を期待する位置に別種のフレームが来た」扱いで GOAWAY を送出し、`last_error_message()` が `Some("Remote peer returned unexpected data while we expected SETTINGS frame.  Perhaps, peer does not support HTTP/2 properly.")` を返す
  - 成立する構成: 両側で `submit_settings()` を明示的に呼び、`want_write()` が偽になるまで `send()` 出力を対面に `recv()` させるのを繰り返す。この構成でサーバー側に `SettingsReceived` と `HeadersReceived` が観測され `get_last_proc_stream_id()` が 1 になること、続けて `submit_response()` が `Ok` を返しクライアント側に `HeadersReceived` と `StreamClosed` が届くことを実測で確認した
- `send()` は 1 回の呼び出しで出力が途中で尽きる (`want_write()` が真のまま返り値が空になることはないが、対面が `recv()` を返した消費長が入力長に届くとは限らない)。ヘルパーは「`while want_write() { send() → recv() }` を双方交互に繰り返す」形で確定させる
- 往復ヘルパーの形を決める際の参照先は `crates/tokio-nghttp2/src/connection.rs` の `Connection::drive()` / `Connection::flush()` / `Connection::recv()` である。ただし `drive()` は `flush()` (1 回の `send()`) と `recv()` を 1 度ずつ行う構成で、`want_write()` ベースのループは持たない (`Session::send()` が `nghttp2_session_mem_send` を返り値 0 まで回して出力を全部 drain するため、tokio 側は 1 回で足りる)。同期テストでも同じ前提に立つことができるが、ヘルパーは `Session` の公開 API (`submit_settings` / `send` / `recv` / `want_read` / `want_write` / `poll_event`) のみで実装し、`nghttp2-sys` の生関数をテストから呼ぶことはしない
- ヘルパーは `test_session` ターゲット内でのみ使うため `tests/helpers/` は作らない (0111 と同じ判断)
- 往復の成立そのものを最初のテストとして固定する (`submit_request()` した HEADERS がサーバー側の `Http2Event::HeadersReceived` として観測されること)。他のサーバー側 API のテストはすべてこのヘルパーに依存する

### API 群ごとの検証方法

- 単一セッションで完結する API (`submit_ping` / `submit_window_update` / `want_read` / `get_outbound_queue_size` / `get_next_stream_id` / `terminate_session`): `send()` 出力と `poll_event()` の `Http2Event::FrameSent { frame_type, .. }`、および返り値で検証する。`submit_window_update()` は 2026-08-28 の実測で、未知のストリーム ID に対しては `Ok` を返すものの送信出力は 0 バイトで `FrameSent` も発生せず、`window_size_increment = 0` も同様に黙って無視された。正常系として `FrameSent { frame_type: FrameType::WindowUpdate, .. }` を得るには connection レベル (`stream_id = 0`) を使うこと。未知ストリーム ID と increment 0 の無視は、それぞれ別テストで「イベントが発生しないこと」として固定する
- `submit_rst_stream()` は送信済みの HEADERS を持つストリームが必須。2026-08-28 の実測では、`submit_request()` 直後 (HEADERS をまだ `send()` していない状態) に `submit_rst_stream(sid, Cancel)` を呼ぶと返り値は `Ok` だが `Http2Event::FrameSent` は発生せず、`Http2Event::FrameNotSent { stream_id, frame_type: FrameType::Headers, lib_error_code: -511 }` (nghttp2 の `NGHTTP2_ERR_STREAM_CLOSING` に相当) と `Http2Event::StreamClosed { stream_id, error_code: ErrorCode::Cancel }` が観測された (送られた HEADERS 自体が取り消されるため、not-send が通知されるフレーム種別は RST_STREAM ではなく HEADERS である)。したがって正常系は、往復を「HEADERS が対面に届いた段階」で止めて (対面からレスポンス受信まで進めるとストリームが閉じ `FrameSent` が得られない) `submit_rst_stream()` を呼び、`Http2Event::FrameSent { frame_type: FrameType::RstStream, stream_id }` の発生で検証する。あわせて上記の HEADERS 送信前の経路を `FrameNotSent` が観測されるテストとして検証する。異常系は `stream_id = 0` (`NGHTTP2_ERR_INVALID_ARGUMENT`。`crates/nghttp2-sys/src/bindings.rs` の `nghttp2_submit_rst_stream` doc に明記されたエラー条件、実測で `-501`) を検証する
- 役割と状態に制約のある送信 API:
  - `submit_shutdown_notice()` はサーバー専用。クライアントセッションで呼ぶと nghttp2 が `NGHTTP2_ERR_INVALID_STATE` を返す (`crates/nghttp2-sys/src/bindings.rs` の `nghttp2_submit_shutdown_notice` doc。`crates/shiguredo_nghttp2/src/session.rs` の doc にはこの制約が書かれていないため、制約は bindings.rs の doc から取る)。正常系は `Session::server()` で、異常系は `Session::client()` で呼んで code が `NGHTTP2_ERR_INVALID_STATE` であることを検証する (2026-08-28 実測で `-519`)
  - `submit_goaway()` の `last_stream_id` はピアのストリーム ID であり、クライアントなら偶数または 0、サーバーなら奇数または 0 でなければならない (`crates/nghttp2-sys/src/bindings.rs` の `nghttp2_submit_goaway` doc。違反は `NGHTTP2_ERR_INVALID_ARGUMENT`)。両ロールで正常系 1 件と、ロールに合わない `last_stream_id` の異常系 1 件を検証する
- `recv()` の正常系: ピア模擬ヘルパー経由の検証に加え、`recv()` の返り値が渡したバイト列の長さと一致すること、および受信したフレーム種別に応じた `Http2Event` (`HeadersReceived` / `DataReceived` / `SettingsReceived` / `PingReceived` / `WindowUpdateReceived` / `GoawayReceived`) が `poll_event()` から取得できることを検証するテストを `tests/test_session.rs` に置く。ヘルパー経由の呼び出しを `recv()` の正常系テストの受け皿として兼ねさせる
- 設定・状態の参照 API (`get_remote_settings` / `get_local_settings` / `get_last_proc_stream_id` / `last_error_message`) は単一セッションだけでは契約を検証できないため、ピア模擬ヘルパーを使った後に検証する
  - `get_remote_settings()`: ピアの SETTINGS を `recv()` した後に広告値が返ることを検証する (未受信時の既定値を assert するだけのテストにしない)
  - `get_local_settings()`: 自側が `submit_settings()` した値が参照できることを検証する
  - `get_last_proc_stream_id()`: クライアントのリクエストをサーバー側で `recv()` した後に、受信したストリーム ID と一致することを検証する (未受信時は初期値が返るだけになる)
  - `last_error_message()`: 生成経路は `session.rs` の `on_error_callback2` に限定される。2026-08-28 の実測で、SETTINGS を期待する位置に別種のフレームを置いて `recv()` させると (サーバー側はプリフェイス消費後の `FIRST_SETTINGS` 状態、クライアント側も自側が SETTINGS を送った直後は同じく SETTINGS 期待状態で、nghttp2 側は同じメッセージを通る)、`last_error_message()` が `Some("Remote peer returned unexpected data while we expected SETTINGS frame.  Perhaps, peer does not support HTTP/2 properly.")` を返すことを確認した。したがって `Some(...)` を返す正常系を「エラーケース」の `recv()` 不正データ経路と兼ねて検証する。`send()` 側で `error_callback2` を発火させる経路は困難なため使わない (`issues/closed/0069-bug-fix-nghttp2-send-set-user-data.md` が同じ理由で `error_callback2` 経路の検証を絞った前例)
- サーバー側で受信ストリームが必須の API (`submit_response` / `submit_headers` / `submit_trailer` / `submit_data_for_trailer`): クライアント側 `submit_request()` の出力をサーバー側 `recv()` に流してストリームを開設してから呼び、`FrameSent` イベントと送信バイト列で検証する。**ピアからストリームを開設させていなくても `submit_response()` は `Ok` を返す (2026-08-28 実測: `Session::server()` の未開設ストリーム ID に対する `submit_response(1, ..., true)` が成功)** ため、返り値の `Ok` だけで正常系と判定しない。必ず `poll_event()` の `Http2Event::FrameSent { frame_type: FrameType::Headers, .. }` の発生まで確認する
  - `submit_headers()` は既存ストリームへの追加 HEADERS 経路のみを検証する。新規ストリーム開始経路 (`stream_id = -1`) は検証対象に含めない。ラッパーは nghttp2 が割り当てたストリーム ID を返さない (`Result<()>`) が、`Http2Event::FrameSent` の `stream_id` では観測できる。それでも対象外とするのは、新規ストリーム開始そのものの検証が `submit_request()` の責務であり重複させる意味が無いためである
- フロー制御 API (`consume` / `consume_connection` / `consume_stream` / `get_remote_window_size` / `get_local_window_size` / `get_stream_remote_window_size` / `get_stream_local_window_size` / `set_local_window_size`): `SessionOptions::no_auto_window_update(true)` を有効にしたセッション (`client_with_options()` / `server_with_options()`) で構築し、DATA 受信後に消費を通知するとウィンドウ値が回復することを実測で検証する
- `SessionOptions` のビルダー 10 個: `no_auto_window_update` は上記のフロー制御検証で兼任し、残り 9 個 (`peer_max_concurrent_streams` / `no_auto_ping_ack` / `max_send_header_block_length` / `max_deflate_dynamic_table_size` / `max_outbound_ack` / `max_settings` / `stream_reset_rate_limit` / `max_continuations` / `glitch_rate_limit`) は `SessionOptions::new()` から連結して設定を保持したまま `client_with_options()` / `server_with_options()` でセッションを構築できることを検証する
- `validation.rs` の 7 関数: 有効入力と無効入力を検証する。空入力の扱いは関数ごとに異なり、以下は現行ビルドの nghttp2 1.69.0 に対する 2026-08-28 の実測値である

  | 関数 | 入力 | 期待値 |
  |---|---|---|
  | `check_header_name` | 空 / `content-length` / 疑似ヘッダー名 `:authority` / 大文字を含む `Content-Length` | `false` / `true` / `true` / `false` |
  | `check_header_value_rfc9113` | 空 / `abc` / 先頭に SP を含む `" a"` / 末尾に SP を含む `"a "` | `true` / `true` / `false` / `false` |
  | `check_method` | 空 / `GET` / 小文字の `get` / 末尾に SP を含む `"GET "` | `false` / `true` / `true` / `false` |
  | `check_path` | 空 / `/` / SP を含む `/a b` / 改行を含む `/a\r\nb` | `true` / `true` / `false` / `false` |
  | `check_authority` | 空 / `example.com` / SP を含む値 / 改行を含む値 | `true` / `true` / `false` / `false` |
  | `http2_strerror` | `ErrorCode::ProtocolError` / `ErrorCode::Unknown(0xFF)` | `"PROTOCOL_ERROR"` / 空にならない |
  | `is_fatal` | `-901` (`NGHTTP2_ERR_NOMEM`) / `-900` (`NGHTTP2_ERR_FATAL`) / `-11` / `0` | `true` / `false` / `false` / `false` |

  `crates/nghttp2-sys/src/bindings.rs` の doc から直接言明できる期待値と、doc が述べておらず nghttp2 の実装に依存する期待値を分けてコメントに記録する。大文字を誤りとする点 (`nghttp2_check_header_name` の "the upper cased alphabet is treated as error")、`nghttp2_check_path` の許容文字が `nghttp2_check_header_value` から SPC と HT を除いた集合である点、`nghttp2_check_header_value_rfc9113` が RFC 9113 Section 8.2.1 を参照する点、`nghttp2_check_method` が RFC 7231 / RFC 7230 のトークン定義を参照する点、`nghttp2_http2_strerror` が未知値に `"unknown"` を返す点、`is_fatal` の境界が `NGHTTP2_ERR_FATAL` 未満である点は、いずれも doc から導ける。一方、**空入力の可否** (`check_path` / `check_authority` が受理し `check_header_name` / `check_method` が拒否する) と、疑似ヘッダー名 `:authority` を受理する点は doc が規定していない実測依存の挙動であり、テストコメントに「nghttp2 の実装に追従している実測値」と明記する
- `crates/shiguredo_nghttp2/src/types.rs` の `Header::new` と `FrameType::from_u8` も本クレートのテストから一度も呼ばれていない公開 API であるため、`tests/test_types.rs` を新設して検証する (`Header::new` の name / value / sensitive の保持、`FrameType::from_u8` は deprecated variant を除く既知値と未知値)

### エラーケース

`crates/shiguredo_nghttp2/src/session.rs` の doc、および doc に制約が書かれていないものは `crates/nghttp2-sys/src/bindings.rs` の doc に明記されたエラー条件に対応づけて追加する。

- `submit_data()` / `submit_data_for_trailer()`: `submit_request(headers, None, true)` 後に resumed プロバイダがない状態で `submit_data()` が `NGHTTP2_ERR_INVALID_ARGUMENT` を返す条件を検証する (`session.rs` の doc「注意」記載)。**失敗後に `send_buffers` へ追加済みのデータが残留する挙動そのものは検証対象にしない** (`send_buffers` は private フィールドで公開 API から観測できず、同じ `stream_id` での再試行による二重化も公開 API からは判別できない)。検証するのは (1) 初回呼び出しが `Err` になること、(2) 続けて `send()` を呼んでも当該データが送信出力に含まれないことまでとする
- `get_stream_remote_window_size()` / `get_stream_local_window_size()`: 未知のストリーム ID で `Error::StreamNotFound` が返ることを検証する (`session.rs` に明示的な分岐があり、`crates/nghttp2-sys/src/bindings.rs` の該当関数 doc は失敗時に -1 を返すと規定する)
- `submit_response()`: クライアントセッションで呼んだ場合 (`NGHTTP2_ERR_PROTO`) と `stream_id = 0` で呼んだ場合 (`NGHTTP2_ERR_INVALID_ARGUMENT`) のエラーを検証する。根拠は `crates/nghttp2-sys/src/bindings.rs` の **`nghttp2_submit_response2`** doc (実装が呼ぶのは v2 側。v1 側は非推奨で、規定内容は同一)。実測では `stream_id = 0` が `-501`、クライアントセッションでの呼び出しが `-505` を返す
- `recv()`: 不正なバイト列 (HTTP/2 プリフェイスでない先頭、フレーム長不整合) を渡した場合に `Err` になることを検証する
- アサーションの粒度: `Error::StreamNotFound` のように本クレートが生成する variant が決まっているものは variant を assert する。nghttp2 起因の `Error::Nghttp2` は原則 `is_err()` のみを assert し、**code を assert することが明記されたケース** (`submit_shutdown_notice` の `NGHTTP2_ERR_INVALID_STATE`、`submit_response` の `NGHTTP2_ERR_PROTO` / `NGHTTP2_ERR_INVALID_ARGUMENT`、`submit_rst_stream` と `submit_goaway` の `NGHTTP2_ERR_INVALID_ARGUMENT`) に限って code を assert する。`submit_data()` / `submit_data_for_trailer()` は doc にコード名が書かれているが、上記の列挙に入れない限り `is_err()` と「送信出力に当該データが含まれないこと」の検証に留める。code の比較は `crates/nghttp2-sys` の定数 (`nghttp2_error_NGHTTP2_ERR_*`、実測で `NGHTTP2_ERR_INVALID_ARGUMENT` = -501 / `NGHTTP2_ERR_PROTO` = -505 / `NGHTTP2_ERR_INVALID_STATE` = -519 / `NGHTTP2_ERR_STREAM_CLOSING` = -511) を使い、数値リテラルは書かない。`nghttp2-sys` は `crates/shiguredo_nghttp2/Cargo.toml` の `[dependencies]` に既にあるため統合テストから参照でき、マニフェスト変更は不要。`Error::Nghttp2` の message 文字列は検証しない (`issues/closed/0069-bug-fix-nghttp2-send-set-user-data.md` が `Err` の variant 種別を assert しない前例)

### 対象外

- `crates/shiguredo_nghttp2/src/error.rs` の `Error::InvalidArgument` / `Error::BufferTooSmall` / `Error::SessionClosed` / `Error::Callback` / `Error::Internal` は本クレート内で生成箇所が無く (grep で確認済み)、テスト追加では CODEBASE.md を満たせない。生成されない variant の削除判断は本 issue の対象外とし、必要なら削除専用の issue で扱う (`issues/closed/0113-change-remove-unused-wt-getters.md` / `issues/closed/0072-change-remove-unused-code.md` が削除側の前例)。そのため CODEBASE.md の充足は本 issue 完了時点では保留となる
- `crates/shiguredo_nghttp2/src/types.rs` の `Http2Event` の variant のうち `InvalidFrameReceived` / `InvalidHeaderReceived` は、本クレートのテストでもワークスペース全体のテストでも一度も観測されていない (生成経路は `session.rs` の `on_invalid_frame_recv_callback` / 不正ヘッダー経路)。発火条件の特定には nghttp2 の内部挙動の調査が必要で、関数・メソッドのテスト追加とは目的が異なるため本 issue では扱わない。`FrameNotSent` は「`submit_rst_stream()`」の項どおりの経路で観測できるため、その検証に含める
- `crates/shiguredo_nghttp2/src/session.rs` の doc に対する追記は本 issue の対象外 (`src/` を変更しない)。`submit_shutdown_notice()` がサーバー専用である制約と `submit_goaway()` の `last_stream_id` の偶奇制約は、現状 `crates/nghttp2-sys/src/bindings.rs` の doc にしか書かれていない。doc へ追記する作業は別の issue で扱う。なお `session.rs` の `submit_shutdown_notice` doc は last_stream_id を `(1u31 << 31) - 1` と表記しており、`1u31 << 31` は型溢出のため正しい表記ではない (`crates/shiguredo_nghttp2/README.md` は `(1 << 31) - 1` で正しい)。この誤記も同じ doc 追記 issue で扱う
- 公開フィールドは本 issue の対象に含めない (`Header::name` / `Header::value` / `Header::sensitive` のような `pub` フィールドの直接検証は行わない。目的で「公開関数・メソッド」と限定しているのはこの意味である)
- `crates/shiguredo_nghttp2/src/error.rs` の `Error::from_nghttp2()` は `session.rs` 内の複数経路から呼ばれており、上記の異常系テストを通じて結果的に検証される。専用のテストは追加しない
- `submit_headers()` の新規ストリーム開始経路 (`stream_id = -1`) は対象外。その検証が `submit_request()` の責務であり重複させる意味が無いためである (`Http2Event::FrameSent` の `stream_id` で観測は可能)
- `FrameType::from_u8` の deprecated な `Priority` (0x02) と `PushPromise` (0x05) は検証対象に含めない。統合テストで variant 名を名指しすると `deprecated` lint が発火し、抑制するには `#[expect(deprecated, ...)]` が必要になるが、`issues/0115-fmt-replace-allow-with-expect.md` が同関数の `#[allow(deprecated)]` を `#[expect(...)]` へ置き換える作業を既に扱っている。`FrameType::from_u8` の検証は非 deprecated の既知値 (`0x00` / `0x01` / `0x03` / `0x04` / `0x06` / `0x07` / `0x08` / `0x09`) と未知値 (`0x0a`) に限定する

## 完了条件

- 「現状」で未検証と列挙した `Session` の公開メソッド・`validation.rs` の 7 関数・`SessionOptions` のビルダー・`types.rs` の `Header::new` / `FrameType::from_u8` の每一项について、正常系 1 件以上のテストが追加されている (`last_error_message()` は「設定・状態の参照 API」に書いた `Some(...)` の検証をもって正常系 1 件とする)
- 「エラーケース」に列挙した 4 条件 (submit_data 系 / ストリーム不存在 / submit_response / recv 不正データ) のテストが追加されている
- 「API 群ごとの検証方法」に書いた役割制約 (`submit_shutdown_notice` はサーバーのみ、`submit_goaway` の `last_stream_id` の偶奇) と `submit_rst_stream()` の HEADERS 対面到着必須・HEADERS 送信前の `FrameNotSent` 観測・`stream_id = 0` 異常系に従うテストが追加されている
- `tests/test_validation.rs` を新設し、`validation.rs` の公開 7 関数について「validation.rs の 7 関数」の表に入力と期待値をそのまま反映したテストが追加されている
- `tests/test_types.rs` を新設し、`Header::new` と `FrameType::from_u8` のテストが追加されている。`FrameType::from_u8` は「対象外」に書いたとおり deprecated variant を検証に含まないため、`test_types.rs` に `#[allow(deprecated)]` も `#[expect(deprecated)]` も現れない
- `SessionOptions` のビルダー 9 個 (フロー制御で兼任する `no_auto_window_update` を除く) について、ビルダー連結後のセッション構築を検証するテストが追加されている
- `crates/shiguredo_nghttp2/src/lib.rs` の既存 `#[cfg(test)]` 11 件に変更がない (移管しない)
- `crates/shiguredo_nghttp2/src/` 配下にコード変更が無い (テストの追加のみ)
- AGENTS.md に従いモック・スタブを導入していない (ピアは実セッション 2 本の同期往復で模擬する)
- 新規テストから `nghttp2_sys` の関数を呼び出していない (`nghttp2_error_NGHTTP2_ERR_*` 定数の参照は可)
- `crates/shiguredo_nghttp2/tests/helpers/` が新規作成されていない (ピア往復ヘルパーは `tests/test_session.rs` 内に置く)
- `CHANGES.md` の `## develop` の `### misc` にテスト追加の `[ADD]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る
- `cargo test -p shiguredo_nghttp2` が全件通過する (短時間確認用。合否判定は `--workspace` で行う)

## 参照

- `crates/shiguredo_nghttp2/src/session.rs` — `Session` の公開メソッドと各 doc に明記されたエラー条件
- `crates/shiguredo_nghttp2/src/validation.rs` — 公開 7 関数
- `crates/shiguredo_nghttp2/src/options.rs` — `SessionOptions` のビルダー
- `crates/shiguredo_nghttp2/tests/test_session.rs` — 既存 4 件
- `crates/tokio-nghttp2/tests/client_server.rs` — 間接カバーしている統合テスト (`test_session_options` ほか)
- `crates/nghttp2-sys/src/bindings.rs` — 検証関数と submit 系関数の引数制約・エラー値の一次情報
- `issues/0111-refactor-split-reset-stream-tests.md` — テスト肥大時のディレクトリモジュール化と `tests/helpers/` を作らない判断 (実装済みの構成前例は `tests/test_hpack/` / `tests/test_stream/` / `tests/test_webtransport/`)
- `issues/0114-test-add-tests-for-unused-public-api.md` — テスト未使用の公開 API をルートクレートで扱う sibling issue。`shiguredo_nghttp2` の検証関数と `SessionOptions` ビルダーは本 issue が対象とする
- `issues/closed/0069-bug-fix-nghttp2-send-set-user-data.md` — 本クレートのテストで `Err` の variant 種別を assert しない粒度の前例
- `issues/closed/0078-change-shiguredo-nghttp2-session-pointer-management.md` — `Session` コンストラクタが `Result<Pin<Box<Session>>>` を返す現在形の経緯
- CODEBASE.md — 公開 API は必ずテストで動作を保証すること
- shiguredo-rust スキル — テスト (公開 API のみ、モック禁止、`tests/test_<module>.rs` 命名、テストファイル分割)
