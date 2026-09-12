# send_max_stream_data が広告した上限を recv_max に反映しない

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-send-max-stream-data-recv-max
- Polished: 2026-09-12

## 目的

`WtSession::send_max_stream_data` がピアへ広告する上限と、ローカルが `WtStream::recv_data` で適用する受信上限 (`WtStream::recv_max`) を一致させる。現在は広告するだけでローカルの上限を更新しないため、ピアが広告どおりにデータを送ると、ローカルが `flow_control_error` でセッションを終了させる。

## 現状

`WtSession::send_max_stream_data` は `maximum` の varint 上限・方向 (`WtStream::has_recv_part`)・受信状態 (`Recv`)・`WtStream::stop_sending_sent` を検証したうえで WT_MAX_STREAM_DATA をエンコードするだけで、`WtStream::recv_max` を更新しない。`recv_max` を更新するのは `WtStream::update_recv_max` を呼ぶ `WtSession::grow_stream_recv_window` だけであり、同じ上限を扱う 2 つの API が非対称になっている。

既定の `WtConfig::default()` のサーバーセッションで、ローカル開始 bidi ストリーム (`recv_max` = `initial_max_stream_data_bidi_local` = 262144) に対して確認した結果は次のとおり。

- `send_max_stream_data(id, 462_144)` の後も `WtStream::recv_max()` は 262144 のまま
- この状態でピアが 262145 bytes (現在の `recv_max` + 1) を送ると `WtSession::process` が `flow_control_error` ("stream recv limit exceeded") を返す

`WtStream::recv_data` は `new_offset > recv_max` を `flow_control_error` とするため、広告値まで正当に送ったピアがエラーになる。広告値 462144 はセッション受信上限 (`initial_max_data` = 1,048,576) の範囲内であり、ストリーム上限が先に評価される。

再現はピアの `WtSession::send_stream_data` では行えない。ピアの `send_max` はローカルが広告した初期値 262144 のままで、262145 bytes を送れないためである。`CapsuleEncoder` で `Capsule::WtStream` を組み立てて `WtSession::feed` へ渡す (既存の `open_peer_stream` ヘルパーと同じ方法)。

また `maximum` が現在の `recv_max` より小さい場合、ピアは draft-ietf-webtrans-http2-15 Section 6.6 の「If an endpoint receives a WT_MAX_STREAM_DATA capsule with a Maximum Stream Data value less than a previously received value, it MUST close the WebTransport session with a WT_FLOW_CONTROL_ERROR session error.」に従いセッションを閉じる。減少を広告しないための検証は現状存在しない。

同じ原因で、`send_max_stream_data` の後に `grow_stream_recv_window` を呼ぶと広告値が減少する。`send_max_stream_data(id, 1_000_000)` の直後に `grow_stream_recv_window(id, 4096)` を呼ぶと、`recv_max` が 262144 のままなので 266240 を広告する。既存テスト `receive_only_stream_recv_operations_accepted` と `bidi_stream_recv_operations_accepted` は「単調増加になるよう、`grow_stream_recv_window` が送る `recv_max` + 4096 より大きい上限を後で送る」というコメントで呼び出し順を調整しており、この制約を前提にしている。

## 設計方針

- `WtStream::recv_max` を「ピアへ最後に広告した上限」の正本とする。SETTINGS で広告した `initial_max_stream_data_*` が初期値であり、`grow_stream_recv_window` も更新と広告を同時に行うため、この不変条件は既存の経路と整合する
- `send_max_stream_data` は `maximum` を検証したうえで、`maximum > recv_max` なら `WtStream::update_recv_max` で `recv_max` を更新してから capsule をエンコードする。`maximum == recv_max` は状態を変えず従来どおり送信する (draft-ietf-webtrans-http2-15 Section 6.6 は冗長な WT_MAX_STREAM_DATA を禁じていない)
- `maximum < recv_max` は `flow_control_error` で拒否し、出力を生成しない。検証は既存の順序 (varint 上限 → ストリーム存在 → 方向 → `stop_sending_sent` → 受信状態) の**後ろ**に置き、既存のエラー種別と優先順位を変えない
- `recv_max` を更新するため、`send_max_stream_data` のストリーム取得を `WtStream` の不変参照から可変参照に変える。検証中は参照を使い回し、`update_recv_max` の後に capsule をエンコードする
- `grow_stream_recv_window` は既存の検証 (方向 / `stop_sending_sent` / 受信状態) を残し、`new_max = recv_max + increment` の計算だけを行って `send_max_stream_data` に更新と広告を任せる。varint 上限は `send_max_stream_data` が capsule のエンコード前に検証する
- `send_max_stream_data` と `grow_stream_recv_window` の doc コメントを、増加のみを受理することと `recv_max` を更新することを含めて更新する
- ドライバ (`DriverState::maybe_grow_stream_window`) は `grow_stream_recv_window` だけを呼び、閾値判定は `recv_available` と設定値から行う。`recv_max` がアプリの広告で大きくなっても、ピアがウィンドウを消費して `recv_available` が `initial.div_ceil(2)` を下回れば従来どおり拡張するため、自動拡張は止まらない。`issues/0157-bug-driver-recv-state-check-size-known.md` が扱う `SizeKnown` の経路では `new_max >= recv_max` のため新しい検証は発火しない
- セッションレベルの `WtSession::send_max_data` / `WtSession::grow_recv_window` にも同じ非対称があるが、本 issue のスコープ外とする

## 完了条件

- `send_max_stream_data` の呼び出し後に `WtStream::recv_max()` が広告値と一致すること
- ピアが広告値 (セッション受信上限の範囲内) まで送っても `WtSession::process` が `flow_control_error` を返さないこと
- `maximum` が現在の `recv_max` より小さい場合に `flow_control_error` を返し、`recv_max` と出力が変化しないこと
- `maximum` が現在の `recv_max` と等しい場合に従来どおり送信されること
- `send_max_stream_data` の後に `grow_stream_recv_window` を呼んでも広告値が減少しないこと
- 方向・受信状態・`stop_sending_sent` の検証順序とエラー種別が非回帰であること (既存テストがそのまま通過すること)
- 既存テスト 2 件 (`receive_only_stream_recv_operations_accepted` / `bidi_stream_recv_operations_accepted`) の呼び出し順のコメントが不要になっていれば削除または更新されていること
- テストが `tests/test_webtransport/integration.rs` に追加され、`cargo test --all` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all --check` が通過すること (`issues/0156-refactor-split-webtransport-integration-tests.md` の分割が先に入った場合は、対応するサブモジュールに追加する)
