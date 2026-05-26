# RFC 参照コメントの CONNECT 言及を修正する

- Priority: Low
- Created: 2026-05-14
- Completed: 2026-05-26
- Model: deepseek-v4-pro
- Branch: feature/fix-rfc-reference-comment

## 目的

`src/validation.rs:303` のコメントが RFC 9113 Section 8.3.1 の `:authority` userinfo 禁止を「http/https/CONNECT に限定」と記載しているが、RFC の原文は「"http" or "https" schemed URIs」のみを対象としており、CONNECT には明示的に言及していない。コメントの RFC 帰属を正確にする。

## 優先度根拠

コード動作自体は防御的で妥当（CONNECT リクエストの `:authority` に userinfo を含むことを拒否するのは安全側）。修正対象はコメントのみであり、機能・安全性に影響しない。

## 現状

`src/validation.rs:303`:

```rust
// RFC 9113 Section 8.3.1: :authority の userinfo 禁止は http/https/CONNECT に限定
```

RFC 9113 Section 8.3.1 (refs/rfc9113.txt L2690-2691) の原文:

> ":authority" MUST NOT include the deprecated userinfo subcomponent for "http" or "https" schemed URIs.

CONNECT リクエストは `:scheme` を持たない (RFC 9113 Section 8.3.1, L2640-2641) ため、この MUST NOT の直接の適用対象外。コードが CONNECT でも userinfo を拒否するのは防御的判断であり、RFC の要件ではない。

## 設計方針

コメントを以下のように修正する:

```rust
// RFC 9113 Section 8.3.1: :authority の userinfo 禁止は http/https に限定
// CONNECT は RFC の適用対象外だが、防御的に同じく拒否する
```

コードの動作（`is_http_scheme || is_connect` の条件）は変更しない。

## 変更対象ファイル

- `src/validation.rs`: コメント修正 (L303)

## 完了条件

- `src/validation.rs:303` のコメントが RFC の記述と正確に一致している
- コードの動作に変更がない
- `cargo test --workspace` が通る
- `cargo clippy --workspace --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る

## 解決方法

`src/validation.rs:303` のコメントを以下のように修正した:

```rust
// RFC 9113 Section 8.3.1: :authority の userinfo 禁止は http/https に限定
// CONNECT は :scheme を持たないため本規定の適用対象外だが、防御的に同じく拒否する
// 注: 将来の RFC 改訂で変更される可能性がある
```

- 「http/https/CONNECT に限定」から CONNECT を除外し、RFC 原文 (L2690-2691) と正確に一致させた
- CONNECT が本規定の適用対象外である理由（`:scheme` を持たないため）を明記した
- AGENTS.md 規約に従い、将来の RFC 改訂で変更される可能性がある旨を追記した
- コードの動作（`is_http_scheme || is_connect` の条件）は変更していない

## 備考: 既に解決済みの項目

本 issue は元々 7 件の RFC 参照誤りを対象としていたが、以下の 6 件は他の issue 対応時に修正済みのため削除した:

1. RFC 9218 Section 5.1 → 2.1 (修正済み)
2. draft Section 6.1 → 6.12 + バージョン番号 (修正済み)
3. RFC 9113 Section 6.9 → 6.9.1 (修正済み)
4. RFC 9110 Section 9 → 9.1 (修正済み)
5. RFC 9110 Section 6.4.1 → RFC 9113 8.1.1 併記 (修正済み)
6. RFC 7540 → RFC 9113 Section 5.3.2 併記 (修正済み)
