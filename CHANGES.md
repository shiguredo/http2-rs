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
- [FIX] クローズ済みの WebTransport ストリームへの WT_STREAM を WT_STREAM_STATE_ERROR として拒否し、新規ストリームとして再作成されないようにする (draft-ietf-webtrans-http2-15 Section 6.4)
  - @voluntas
- [FIX] STOP_SENDING 送信後にピアから在路データが届いても WebTransport セッションが abort されず、停止要求後の受信データをアプリへ配送しないようにする (RFC 9000 Section 3.5)
  - @voluntas
- [FIX] WebTransport ドライバの自動ウィンドウ拡張がローカル開始 bidi ストリームで `initial_max_stream_data_bidi_local` を基準に動作するようにする (draft-ietf-webtrans-http2-15 Section 11.2)
  - @voluntas
- [FIX] WebTransport セッションが Closed 状態に遷移した後は受信 capsule を無視し、新規ストリームを生成しないようにする (draft-ietf-webtrans-http2-15 Section 6.12)
  - @voluntas

### misc
