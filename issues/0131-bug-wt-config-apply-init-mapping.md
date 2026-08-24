# WtConfig::apply_init の bl / br マッピングが仕様と不一致

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-config-apply-init-mapping
- Polished: 2026-08-24

## 目的

`WtConfig::apply_init` (`src/webtransport.rs` の `WtConfig` 型のメソッド) の `bl` / `br` のマッピングが draft-ietf-webtrans-http2-15 Section 4.3.2 の定義と不一致であり、誤った意味論を公開している問題を修正する。現在はテスト専用の使用だが、実セッションで誤用された場合にフロー制御値が誤って適用される。

## 現状

draft-ietf-webtrans-http2-15 Section 4.3.2 の WebTransport-Init ヘッダーのキー定義は以下のとおり:

- `u`: ヘッダー送信者から見て、受信者が開く単方向ストリームの初期フロー制御上限
- `bl`: ヘッダー送信者が開く双方向ストリームの初期フロー制御上限
- `br`: ヘッダー受信者が開く双方向ストリームの初期フロー制御上限

WebTransport-Init はクライアントが CONNECT リクエストで送信するヘッダーであり、その値は「送信者 (クライアント) が課す受信上限」を意味する。したがって受信側 (サーバー) は、これをピア用 config (`apply_init_as_peer`) にマージすべきであり、`tokio-http2` の `WtServerRequest::accept` は正しく `apply_init_as_peer` を使用している。

一方、`WtConfig::apply_init` はローカル config にマージする設計であり、`bl` → `initial_max_stream_data_bidi_remote`、`br` → `initial_max_stream_data_bidi_local` と逆マッピングしている。このメソッドは送信側視点でも受信側視点でも仕様と一致しない。

既存テスト (`tests/test_webtransport/init.rs`) は config と peer_config を同一値で構築して検証しているため、bl / br の逆マッピングを検出できていない。

## 設計方針

- `WtConfig::apply_init` は意味論が曖昧なメソッドであり、削除して `apply_init_as_peer` に一本化する (WebTransport-Init は送信者 (クライアント) の受信上限を伝えるもので、ローカル config へのマージに正当な用途がなく、現状もテスト専用の使用)
- 残す場合は、送信者視点のマッピング (bl → `initial_max_stream_data_bidi_local`、br → `initial_max_stream_data_bidi_remote`) に修正する。これは `apply_init_as_peer` と同一のマッピングになるため、実質的に削除と等価である
- テストを「config と peer_config が異なる値」のケースに変更し、bl / br の各分岐を実値で検証する

## 完了条件

- `WtConfig::apply_init` が削除され、`apply_init_as_peer` に統一されていること (残す場合は、bl → `initial_max_stream_data_bidi_local`、br → `initial_max_stream_data_bidi_remote` の送信者視点マッピングに修正されていること)
- config と peer_config を異なる値で構築した bl / br 検証テストが追加されていること
- `cargo test --all` が通過すること
