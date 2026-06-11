# RFC 9297 非最小エンコーディング拒否の扱いを決定する

- Priority: Medium
- Created: 2026-06-11
- Polished: {Polished}
- Model: deepseek-v4-pro
- Branch: feature/change-rfc9297-non-minimal-varint

## 目的

`src/webtransport/varint.rs` の QUIC 可変長整数デコーダーが非最小エンコーディングを拒否する挙動について、RFC 9297 準拠の方針を決定し実装する。

## 現状の問題

`src/webtransport/varint.rs:192-201`:

```rust
// RFC 9000 Section 16 は Frame Type を除き最小エンコーディングを要求しないが、
// 本実装は独自方針として非最小エンコーディングを拒否する
if encoded_len(value) != len {
    return Err(WtError::with_reason(
        WtErrorKind::InvalidInput,
        format!("non-minimal varint encoding: value {value} encoded in {len} bytes, minimum is {}",
            encoded_len(value)
        ),
    ));
}
```

RFC 9297 Section 1.1 は Capsule Protocol 向けに明示している:

> Integer values do not need to be encoded on the minimum number of bytes necessary.

コードのコメントは「本実装独自の厳格化」としているが:
- RFC 9297 に準拠した実装が非最小エンコーディングで Capsule Type や Capsule Length を送信した場合、本実装はそれを不正として拒否する
- コメントは RFC 9000（QUIC）に言及しているが、RFC 9297 Section 1.1 の許容文言への言及がない
- 「独自方針」の根拠（DoS 耐性か、コード簡略化か）が不明

## 完了条件

以下のいずれかの方針が決定・実装されていること:

**方針 A: RFC 9297 に準拠する**
- 非最小エンコーディングの検査 (`encoded_len(value) != len`) を削除
- 該当コメントを削除
- 非最小エンコーディングの受入テストを追加

**方針 B: 意図的逸脱として明記する**
- コメントを拡充し以下を明記:
  - RFC 9297 Section 1.1 の "do not need to be encoded on the minimum number of bytes necessary" を引用
  - 意図的に逸脱する理由（例: DoS 耐性のため、コード簡略化のため）
  - この逸脱が引き起こす相互運用性リスク
- CHANGES.md `## develop` に `[CHANGE]` エントリを追加すること

## 参照

- `src/webtransport/varint.rs:192-201` — 非最小エンコーディング拒否の実装
- `refs/rfc9297.txt` — RFC 9297 Section 1.1
- `refs/draft-ietf-webtrans-http2-14.txt` — WebTransport over HTTP/2（Capsule Protocol の利用元）
