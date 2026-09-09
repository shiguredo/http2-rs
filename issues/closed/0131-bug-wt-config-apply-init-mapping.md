# WtConfig::apply_init の bl / br マッピングが仕様と不一致

- Created: 2026-08-24
- Completed: 2026-09-09
- Branch: feature/fix-wt-config-apply-init-mapping
- Polished: 2026-09-09

## 目的

`WtConfig::apply_init` (`src/webtransport.rs` の `WtConfig` 型のメソッド) の `bl` / `br` のマッピングが draft-ietf-webtrans-http2-15 Section 4.3.2 の定義と不一致であり、誤った意味論を公開している問題を修正する。現在はテスト専用の使用だが、実セッションで誤用された場合にフロー制御値が誤って適用される。

## 現状

draft-ietf-webtrans-http2-15 Section 4.3.2 の WebTransport-Init ヘッダーのキー定義は以下のとおり:

- `u`: ヘッダー送信者から見て、受信者が開く単方向ストリームの初期フロー制御上限
- `bl`: ヘッダー送信者が開く双方向ストリームの初期フロー制御上限
- `br`: ヘッダー受信者が開く双方向ストリームの初期フロー制御上限

WebTransport-Init はクライアントが CONNECT リクエストで送信するヘッダーであり、その値は「送信者 (クライアント) が課す受信上限」を意味する。したがって受信側 (サーバー) は、これをピア用 config (`apply_init_as_peer`) にマージすべきであり、`tokio-http2` の `WtServerRequest::accept` は正しく `apply_init_as_peer` を使用している。

一方、`WtConfig::apply_init` はローカル config にマージする設計であり、`bl` → `initial_max_stream_data_bidi_remote`、`br` → `initial_max_stream_data_bidi_local` と逆マッピングしている。このメソッドは送信側視点でも受信側視点でも仕様と一致しない。

既存テスト (`tests/test_webtransport/init.rs`) の `test_apply_init_max_merge` は `bl` → `initial_max_stream_data_bidi_remote`、`br` → `initial_max_stream_data_bidi_local` を直接 assert しており、現行の誤ったマッピングを期待値として固定している。伝搬テスト (`test_apply_init_propagates_*`) は config と peer_config を同一値で構築するため、マッピングの誤りを検出できない。`apply_init` の削除時にはこれらの既存テストも削除または書き換えが必要である。

## 設計方針

- `WtConfig::apply_init` は意味論が曖昧なメソッドであり、削除して `apply_init_as_peer` に一本化する (WebTransport-Init は送信者 (クライアント) の受信上限を伝えるもので、ローカル config へのマージに正当な用途がなく、現状もテスト専用の使用)。マッピングだけを送信者視点に反転してローカル config へ適用し続けると、送信者の上限を受信側自身の受信上限に書き込む誤実装になるため、残す案は採らない
- `apply_init` の削除に伴い、`apply_init_as_peer` の doc、`overlay_settings` の rustdoc リンク `[Self::apply_init]`、`crates/tokio-http2` のコメント、`tests/test_webtransport/init.rs` のモジュール doc、`skills/shiguredo-http2/SKILL.md` の `apply_init` 記載も更新する
- `apply_init_as_peer` の bl / br の各分岐を実値で直接検証するテストを追加し、既存の `apply_init` を使うテスト (`tests/test_webtransport/init.rs` の `test_apply_init_*`) は削除または `apply_init_as_peer` 向けに書き換える

## 完了条件

- `WtConfig::apply_init` が削除され、`apply_init_as_peer` に統一されていること
- `apply_init_as_peer` の `bl` → `initial_max_stream_data_bidi_local`、`br` → `initial_max_stream_data_bidi_remote` の各分岐を実値で直接検証するテストが追加されていること。既存の `apply_init` を使うテストは削除または書き換えられていること
- `apply_init` を参照する doc・rustdoc リンク・コメント・`skills/shiguredo-http2/SKILL.md` が更新されていること
- `cargo test --all` が通過すること

## 解決方法

- `src/webtransport.rs` の `WtConfig::apply_init` を削除し、WebTransport-Init のマージを `apply_init_as_peer` に一本化した
- `apply_init_as_peer` の doc を更新し、`u` / `bl` / `br` のマッピング (`bl` → `initial_max_stream_data_bidi_local`、`br` → `initial_max_stream_data_bidi_remote`) を明記した。`overlay_settings` の rustdoc リンクも `apply_init_as_peer` に更新した
- `tests/test_webtransport/init.rs` の `test_apply_init_*` を `apply_init_as_peer` 向けに書き換え、`bl` と `br` に異なる値を与えてマッピングの取り違えを検出できるようにした。小さい値が無視されることも `u`/`bl`/`br` の 3 キーで検証する
- `crates/tokio-http2` のコメントと `skills/shiguredo-http2/SKILL.md` の `apply_init` 記載を更新した
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリを追加した (公開 API 削除のため)
