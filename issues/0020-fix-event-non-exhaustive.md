# Event enum に #[non_exhaustive] を付与し CHANGES.md 種別を修正する

Created: 2026-05-14
Model: deepseek-v4-pro

## 対象

- `src/event.rs`
- `CHANGES.md`

## 内容

### 1. `Event` enum に `#[non_exhaustive]` が付いていない

`Event` enum は公開型だが `#[non_exhaustive]` 属性がない。`Event::HeadersReceived` に `protocol: Option<Vec<u8>>` フィールドを追加した変更は、外部クレートのパターンマッチングを破壊する後方互換性のない変更である。

`#[non_exhaustive]` を付与することで、将来のバリアント追加が後方互換を破壊しないようにする。

### 2. CHANGES.md の種別が不適切

CHANGES.md の `## develop` セクションには:

```
- [ADD] `shiguredo_http2::Event::HeadersReceived` に Extended CONNECT の `:protocol` 値を伝搬する `protocol: Option<Vec<u8>>` フィールドを追加する
```

とあるが、enum バリアントへのフィールド追加は後方互換を破壊する変更であり、本来は `[CHANGE]` が適切。`#[non_exhaustive]` を付与する場合は `[ADD]` のままで問題ないが、付与しない場合は `[CHANGE]` に修正する必要がある。

## 修正方針

1. `Event` enum に `#[non_exhaustive]` を付与する
2. 今回の `protocol` フィールド追加が実質後方互換を保つことになるため、CHANGES.md の種別は `[ADD]` のままとする
