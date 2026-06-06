# check_header_block_fragment_size が max_header_list_size=None 時に無効で CONTINUATION フラッド DoS が成立する問題を修正する

- Priority: High
- Created: 2026-06-06
- Model: DeepSeek V4 Pro

## 目的

`src/connection/headers.rs:719-729` の `check_header_block_fragment_size` は、`max_header_list_size` が `None`（無制限）の場合に**常に `Ok(())` を返す**。このとき攻撃者が END_HEADERS なしの CONTINUATION フレームを無限に送信することで `header_block_fragment` が無制限に拡張され、メモリ枯渇 DoS（CVE-2016-8740 系）が成立する。

## 優先度根拠

- CVE-2016-8740 / CVE-2019-9515 と同根の脆弱性
- `Settings::default()` の `max_header_list_size` が `None` であるため、明示的に上限を設定しない限り**全接続が攻撃対象**
- RFC 9113 §6.10 は CONTINUATION フレームの個数に上限を設けておらず、フレーム単体のサイズ制限 (`max_frame_size`) だけでは累積サイズを抑制できない

## 現状

`src/settings.rs:27`:

```rust
pub const DEFAULT_MAX_HEADER_LIST_SIZE: Option<u32> = None;
```

`src/connection/headers.rs:719-729`:

```rust
fn check_header_block_fragment_size(&self) -> Result<()> {
    if let Some(max_size) = self.local_settings.max_header_list_size()
        && self.header_block_fragment.len() > max_size as usize
    {
        return Err(Error::connection_error(
            ErrorCode::CompressionError,
            "accumulated header block fragment exceeds SETTINGS_MAX_HEADER_LIST_SIZE",
        ));
    }
    Ok(())
}
```

`None` の場合は else 節がなく常に `Ok(())` を返す。

## 設計方針

`None` 時にも絶対的な上限を適用する。以下のいずれか:

1. **固定上限**: `None` 時に接続あたりの絶対上限（例: 64MB）を設ける
2. **デフォルト値変更**: `DEFAULT_MAX_HEADER_LIST_SIZE` を `None` から `Some(16384)` 等の安全な値に変更する（`Limits::builder()` のデフォルトと一致させる）

RFC 9113 §10.5.1 が上限設定を強く推奨しており、多くの実装は 8KB〜64KB をデフォルト値としている。

## 完了条件

- `max_header_list_size = None` 時にも `header_block_fragment` のサイズに絶対的な上限が適用される
- CONTINUATION フレームを無制限に送信してもメモリが枯渇しない
- 既存のヘッダー継続テストが通過する
- 新規に上限超過を検証するテストが追加されている

## 解決方法

1. 作業ブランチ `feature/fix-continuation-flood` を切る
2. `DEFAULT_MAX_HEADER_LIST_SIZE` を `Some(16384)` に変更する（`Limits::builder()` と一貫させる）
3. または `check_header_block_fragment_size` に固定上限を追加する
4. テストを追加し `cargo test --all` で全通過を確認する
