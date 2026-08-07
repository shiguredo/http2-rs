# 明示 reset_stream が idle ストリームに RST_STREAM を送信する問題を修正する

- Priority: Medium
- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-reset-stream-idle-stream
- Polished: 2026-08-07

## 目的

`Connection::reset_stream` が一度も開かれていない idle ストリームへの明示呼び出しで RST_STREAM フレームを送信する。RFC 9113 Section 6.4 は idle ストリームへの RST_STREAM 送信を MUST NOT で禁止しており、受信したピアがそのストリームを idle とみなす場合は PROTOCOL_ERROR の接続エラーにし、接続全体が終了してしまう (RFC 9113 Section 5.1 のとおりストリーム状態は主観的なため、ピアが既に open とみなしている場合は通常のリセットとして処理される)。

## 現状

`src/connection.rs` の `Connection::reset_stream` は `StreamId::Connection` (stream_id = 0) のみを拒否し、`streams` に存在しないストリーム ID に対しては無条件に RST_STREAM フレームを送信する。`streams` に存在しないストリームには「クローズ済み」と「idle (一度も開かれていない)」の 2 種類があり、後者への RST_STREAM 送信は RFC 9113 Section 6.4 の MUST NOT 違反である。

## 設計方針

`Connection::reset_stream` の送信前に idle ストリームの検査を追加し、idle と判定された場合は RST_STREAM を送信せず `Error::stream_error(ErrorCode::ProtocolError, ...)` を返す。

- idle 判定には `is_idle_stream` を使用する。`is_idle_stream` は issue 0099 (`issues/0099-bug-fix-internal-reset-stream-event.md`) の修正で `closed_streams` を考慮するようになるため、その修正の後に実装する (クローズ済みストリームを idle と誤判定しないため)。内部呼び出し 3 箇所 (`Connection::process` / `Connection::handle_data` / `Connection::handle_window_update`) はすべて非 idle のストリームに対してのみ呼ばれるため、この変更の影響を受けない
- idle 検査は `StreamId::Connection` (stream_id = 0) の拒否の後に配置する。`is_idle_stream` は偶数 ID を idle と判定するため、検査を先に置くと stream_id = 0 が `Error::stream_error` にすり替わり、既存の接続エラー (PROTOCOL_ERROR) の挙動が変わってしまう
- エラー種別は公開 API の呼び出し拒否で使われる `Error::stream_error` を踏襲する (例: `send_data` のクローズ済みストリーム拒否)。`Connection::process` の受信経路が idle ストリームのストリームエラーを接続エラーに昇格するのは、受信フレームのエラー処理であり本 API 呼び出しの拒否とは状況が異なる。`ErrorKind::StreamError` の意味論 (「RST_STREAM を送信する必要がある」) とは厳密には一致しないが、これは MUST NOT により送信できないケースであり、接続エラー (GOAWAY を要求する意味論) とは区別する
- クローズ済みストリーム (「`streams` に存在しない」かつ「idle でない」) への明示リセットは既存挙動 (RST_STREAM 送信のみ) を維持する。ただし `closed_streams` の上限 (10000 件) を超過して追い出されたクローズ済みストリームのうち `last_recv_stream_id` を超えるものは、`is_idle_stream` の判定上 idle とみなされエラーになる (既知の限界。既存の `check_not_idle_stream` と同種の制約)。また、クローズ済みストリームへの RST_STREAM 送信は RFC 9113 Section 5.1 の closed 状態へのフレーム送信制限 (MUST NOT send frames other than PRIORITY on a closed stream) に厳密には抵触しうるが、本修正は idle ストリームへの送信禁止のみを対象とし、クローズ済みへの送信は既存挙動を維持する (スコープ外)
- issue 0099 の設計方針「`streams` に存在しないストリーム (クローズ済み・idle) への呼び出しでは既存挙動 (RST_STREAM 送信のみ) を維持する」のうち idle 部分は、本修正で「エラーを返す」挙動に変更される

## 完了条件

- idle ストリーム ID (`is_idle_stream` が idle と判定するストリーム ID) を `Connection::reset_stream` に渡すと RST_STREAM を送信せず `Error::stream_error` (PROTOCOL_ERROR) を返す
- `StreamId::Connection` (stream_id = 0) は従来どおり `Error::connection_error` (PROTOCOL_ERROR) を返す
- クローズ済みストリームへの明示リセットは既知の限界 (`closed_streams` の上限超過で追い出されたクローズ済みストリームのうち `last_recv_stream_id` を超えるものは `is_idle_stream` が idle と判定する) を除き、既存挙動 (RST_STREAM 送信のみ) を維持する
- 上記を検証するテストが追加され、全テストが通過する

## 解決方法

1. `src/connection.rs` の `Connection::reset_stream` に idle ストリームの検査を追加する (配置は `StreamId::Connection` 拒否の後、RST_STREAM 送信の前)
2. テストを追加する (`tests/test_connection.rs` に単体テストとして追加する。いずれも意図的なエラーパスの検証であり、PBT (`pbt/`) ではなく単体テストで検証する):
   - idle ストリーム ID に `reset_stream` を呼ぶとエラーになり、RST_STREAM が送信されないことを検証する (判定分岐の 2 系統: `last_recv_stream_id` 超過の奇数 ID / 偶数 ID)
   - `StreamId::Connection` に `reset_stream` を呼ぶと従来どおり接続エラー (PROTOCOL_ERROR) を返すことを検証する (idle 検査の配置による退行の防止)
   - クローズ済みストリームへの `reset_stream` が既存挙動を維持することを検証する (クローズ済みストリームは受信 RST_STREAM で `streams` から削除されたストリームなどで用意する。クライアントロールで `last_recv_stream_id` を超える ID のストリームを使う場合は、0099 の `is_idle_stream` 修正 (closed_streams 考慮) に依存する)
   - ピアがストリーム ID を飛ばしたことで暗黙的にクローズ済みになったストリーム (例: サーバーロールでストリーム 1 と 5 の HEADERS を受信後、ストリーム 3 への明示リセット。RFC 9113 Section 5.1.1 の暗黙クローズ) も、既存挙動 (RST_STREAM 送信のみ) を維持することを検証する
3. issue 0099 で追加された「クローズ済み・idle ストリームへの明示 `reset_stream` で `Event::StreamReset` が push されないこと」を検証するテストのうち、idle ケースが検証対象とする実装挙動は本修正で Err を返す挙動に変わる。0099 実装時は idle ケースで `reset_stream` の成功 (Ok) や RST_STREAM 送信を assert しないこととし、assert していた場合は本修正で Err 前提のテストへ修正する。なお、0099 の「リセット済みストリームへの遅延 DATA 破棄」テストはリセット対象が `streams` に存在するストリームであり、本修正 (idle 検査) の影響を受けない
4. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する (shiguredo-changelog スキルを参照)
5. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` を実行する

## 参照

- `src/connection.rs` — `Connection::reset_stream` / `is_idle_stream`
- `refs/rfc9113.txt` — Section 6.4 (idle ストリームへの RST_STREAM 送信禁止) / Section 5.1 (idle・closed 状態の定義) / Section 5.1.1 (スキップされたストリーム ID の暗黙クローズ) / Section 5.4.2 (RST_STREAM の複数送信制限)
- `issues/0099-bug-fix-internal-reset-stream-event.md` — `is_idle_stream` に `closed_streams` の参照を追加する先行修正。本修正は 0099 の後に実装する
