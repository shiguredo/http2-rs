# send_max_stream_data が広告した上限を recv_max に反映しない

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-send-max-stream-data-recv-max
- Polished: {YYYY-MM-DD}

## 目的

`WtSession::send_max_stream_data` がピアへ広告する上限と、ローカルが `WtStream::recv_data` で適用する受信上限 (`WtStream::recv_max`) を一致させる。現在は広告するだけでローカルの上限を更新しないため、ピアが広告どおりにデータを送ると、ローカルが `flow_control_error` でセッションを終了させる。

## 現状

`WtSession::send_max_stream_data` は `maximum` の varint 上限・方向 (`WtStream::has_recv_part`)・受信状態 (`Recv`)・`WtStream::stop_sending_sent` を検証したうえで WT_MAX_STREAM_DATA をエンコードするだけで、`WtStream::recv_max` を更新しない。`recv_max` を更新するのは `WtStream::update_recv_max` を呼ぶ `WtSession::grow_stream_recv_window` だけであり、同じ上限を扱う 2 つの API が非対称になっている。

既定の `WtConfig::default()` のサーバーセッションで、ローカル開始 bidi ストリームに対して実測した結果は次のとおり。

- `send_max_stream_data(id, recv_available() + 1_000_000)` の後も `WtStream::recv_available()` は 262144 のまま
- この状態でピアが 262145 bytes (現在の `recv_max` + 1) を送ると `WtSession::process` が `flow_control_error` ("stream recv limit exceeded") を返す

`WtStream::recv_data` は `new_offset > recv_max` を `flow_control_error` とするため、広告値まで正当に送ったピアがエラーになる。

また `maximum` が現在の `recv_max` より小さい場合、ピアは draft-ietf-webtrans-http2-15 Section 6.6 の「If an endpoint receives a WT_MAX_STREAM_DATA capsule with a Maximum Stream Data value less than a previously received value, it MUST close the WebTransport session with a WT_FLOW_CONTROL_ERROR session error.」に従いセッションを閉じる。減少を広告しないための検証は現状存在しない。

## 設計方針

- `WtStream::recv_max` を「ピアへ最後に広告した上限」の正本とする。SETTINGS で広告した `initial_max_stream_data_*` が初期値であり、`grow_stream_recv_window` も更新と広告を同時に行うため、この不変条件は既存の経路と整合する
- `send_max_stream_data` は `maximum` を検証したうえで、`maximum > recv_max` なら `WtStream::update_recv_max` で `recv_max` を更新してから capsule をエンコードする。`maximum == recv_max` は状態を変えず従来どおり送信する (draft-ietf-webtrans-http2-15 Section 6.6 は冗長な WT_MAX_STREAM_DATA を禁じていない)
- `maximum < recv_max` は `flow_control_error` で拒否し、出力を生成しない。ピアが MUST でセッションを閉じる値のため、送信前にローカルで拒否する
- `grow_stream_recv_window` は現在 `recv_max` を更新してから `send_max_stream_data` を呼ぶ。更新を `send_max_stream_data` に一本化し、`new_max` の計算だけを残す。varint 上限 (2^62-1) の検証は現行どおり capsule のエンコード前に行う
- 方向 (`has_recv_part` / 0147)、受信状態 (`Recv` / 0149)、`stop_sending_sent` (Section 6.6 の MUST NOT) の検証は非回帰とする

## 完了条件

- `send_max_stream_data` の呼び出し後に `WtStream::recv_available()` が広告値と一致すること
- ピアが広告値まで送っても `WtSession::process` が `flow_control_error` を返さないこと
- `maximum` が現在の `recv_max` より小さい場合に `flow_control_error` を返し、`recv_max` と出力が変化しないこと
- `maximum` が現在の `recv_max` と等しい場合に従来どおり送信されること
- `grow_stream_recv_window` の増加量の加算と広告が非回帰であること
- テストが追加され、`cargo test --all` が通過すること
