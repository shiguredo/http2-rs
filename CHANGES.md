# 変更履歴

- UPDATE
  - 後方互換がある変更
- ADD
  - 後方互換がある追加
- CHANGE
  - 後方互換のない変更
- FIX
  - バグ修正

## develop

- [CHANGE] `WtConfig::apply_init` を削除し、WebTransport-Init のマージを `apply_init_as_peer` に一本化する (draft-ietf-webtrans-http2-15 Section 4.3.2)
  - @voluntas
- [ADD] `shiguredo_http2::Connection` / `tokio_http2::Connection` / `tokio_http2::ServerConnection` に、ストリームの送信待ちデータの有無を返す `has_pending_send_data` を追加する
  - @voluntas
- [ADD] `WtConfig` に、ピア用の初期フロー制御値を仕様の Initial Value (全て 0) で生成する `peer_default()` を追加する (draft-ietf-webtrans-http2-15 Section 11.2)
  - @voluntas
- [ADD] `WtStream` に、ストリームが受信パートを持つかどうかを方向 (双方向またはピア開始) で判定する `has_recv_part` を追加する
  - @voluntas
- [FIX] クローズ済みの WebTransport ストリームへの WT_STREAM を WT_STREAM_STATE_ERROR として拒否し、新規ストリームとして再作成されないようにする (draft-ietf-webtrans-http2-15 Section 6.4)
  - @voluntas
- [FIX] STOP_SENDING 送信後にピアから在路データが届いても WebTransport セッションが abort されず、停止要求後の受信データをアプリへ配送しないようにする (RFC 9000 Section 3.5)
  - @voluntas
- [FIX] WebTransport ドライバの自動ウィンドウ拡張がローカル開始 bidi ストリームで `initial_max_stream_data_bidi_local` を基準に動作するようにする (draft-ietf-webtrans-http2-15 Section 11.2)
  - @voluntas
- [FIX] WebTransport セッションが Closed 状態に遷移した後は受信 capsule を無視し、新規ストリームを生成しないようにする (draft-ietf-webtrans-http2-15 Section 6.12)
  - @voluntas
- [FIX] 通常の CONNECT リクエストの :authority 検証で host 部 (uri-host) を RFC 3986 の reg-name 文字集合と pct-encoded で検査し、SP・制御文字・非 ASCII・不正な pct-encoded・IPv6 リテラル内部の不正文字を拒否する (RFC 9113 Section 8.5 / RFC 9112 Section 3.2.3 / RFC 3986 Section 3.2.2)
  - @voluntas
- [FIX] `Connection::send_response` / `Connection::send_trailers` が送信バッファに滞留 DATA がある状態で END_STREAM 付き HEADERS を送るのを拒否し、データの無通知消失を防ぐ (RFC 9113 Section 8.1)
  - @voluntas
- [FIX] SETTINGS_INITIAL_WINDOW_SIZE の増加でストリーム送信ウィンドウが拡張された後、滞留していた送信 DATA をフラッシュする (RFC 9113 Section 6.9.2)
  - @voluntas
- [FIX] `Connection::send_goaway` を複数回呼び出しても last-stream-id が増加しないようにする (RFC 9113 Section 6.8)
  - @voluntas
- [FIX] 送信バッファ容量をピアの SETTINGS_INITIAL_WINDOW_SIZE から分離して 65535 に固定し、バッファ超過を接続エラーではなくストリームエラーとして返す。1 回の `send_data` で渡せるのは 65535 bytes までとなり、それ以上は呼び出し側で分割する必要がある (RFC 9113 Section 6.9)
  - @voluntas
- [FIX] 送信ウィンドウ枯渇時に `WtServerSession::close()` / `WtSessionHandle::close()` が Ok を返しても WT_CLOSE_SESSION / END_STREAM が送信されない場合はエラーを返し、呼び出し側が未送信を認識できるようにする (draft-ietf-webtrans-http2-15 Section 6.12)
  - @voluntas
- [FIX] ローカル開始 uni ストリーム (送信専用) への WT_STREAM を WT_STREAM_STATE_ERROR として拒否し、送信専用ストリームにピアデータが配送されないようにする (draft-ietf-webtrans-http2-15 Section 6.4 / RFC 9000 Section 2.1 / Section 19.8)
  - @voluntas
- [FIX] WebTransport の自動ウィンドウ拡張のしきい値を切り上げ除算にし、`initial_max_data` / `initial_max_stream_data_*` が 1 でも受信ウィンドウが拡張されるようにする (draft-ietf-webtrans-http2-15 Section 6.5 / Section 6.6)
  - @voluntas
- [FIX] WebTransport ドライバが WT 出力を送信バッファの固定容量 (65535 bytes) 以下に分割して送信し、ピアが十分な送信ウィンドウを広告する構成で 65535 bytes を超えるストリーム送信が失敗しないようにする (draft-ietf-webtrans-http2-15 Section 2 / RFC 9113 Section 6.9)
  - @voluntas
- [FIX] Extended CONNECT の `:authority` 検証で host 部 (uri-host) を RFC 3986 の reg-name 文字集合と pct-encoded で検査し、SP・制御文字・非 ASCII・不正文字・不正な pct-encoded・IPv6 リテラル内部の不正文字を拒否する。ポートは省略可能とし、指定時は数字のみで 0-65535 の範囲に制限する (RFC 8441 Section 4 / RFC 9113 Section 8.3.1 / RFC 3986 Section 3.2 / Section 3.2.2 / Section 3.2.3)
  - @voluntas
- [FIX] WebTransport セッション確立時のピア用 `WtConfig` を仕様の Initial Value 0 から構築し、`SETTINGS_WT_INITIAL_MAX_*` を広告しないピアの未広告項目が `WtConfig::default` の既定値にならないようにする (draft-ietf-webtrans-http2-15 Section 4.3.1 / Section 11.2)
  - @voluntas
- [FIX] WebTransport の `send_stream_data` がセッション送信ウィンドウ超過時にストリームの送信済みバイト数と送信状態を更新してしまう問題を修正し、拒否時に部分的な状態変更を残さないようにする (draft-ietf-webtrans-http2-15 Section 6.5 / Section 6.6)
  - @voluntas
- [FIX] 送信専用のローカル開始 uni ストリームへの WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED を WT_STREAM_STATE_ERROR として拒否し、WT_RESET_STREAM 受信時に `StreamReset` イベントが誤って送出されないようにする (draft-ietf-webtrans-http2-15 Section 6.2 / Section 6.9 / RFC 9000 Section 19.4 / Section 19.13)
  - @voluntas

### misc
