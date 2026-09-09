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

- [FIX] クローズ済みの WebTransport ストリームへの WT_STREAM を WT_STREAM_STATE_ERROR として拒否し、新規ストリームとして再作成されないようにする (draft-ietf-webtrans-http2-15 Section 6.4)
  - @voluntas

### misc
