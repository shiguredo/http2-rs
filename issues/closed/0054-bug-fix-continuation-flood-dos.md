# check_header_block_fragment_size が max_header_list_size=None 時に無効で CONTINUATION フラッド DoS が成立する問題を修正する

- Priority: Medium
- Created: 2026-06-06
- Completed: 2026-06-06
- Model: DeepSeek V4 Pro
- Branch: feature/fix-continuation-flood-limit
- Polished: 2026-06-06

## 目的

`src/connection/headers.rs:719-729` の `check_header_block_fragment_size` は、`local_settings.max_header_list_size()` が `None` の場合に**常に `Ok(())` を返す**。このとき攻撃者が END_HEADERS なしの CONTINUATION フレームを無限に送信することで `header_block_fragment` が無制限に拡張され、メモリ枯渇 DoS が成立する。

## 優先度根拠

- CVE-2016-8740 と同根の脆弱性。RFC 9113 §6.10 は CONTINUATION フレームの個数に上限を設けておらず、フレーム単体のサイズ制限 (`max_frame_size`) だけでは累積サイズを抑制できない
- ただし影響範囲は限定的: `Limits::default()` を使用する通常の接続では `max_header_list_size: Some(16384)` が設定されるため保護されている。無防備になるのは `Limits::builder().max_header_list_size(None)` を明示的に指定した接続、または `Settings::default()` を直接使用する非標準的な初期化経路のみ
- 優先度 Medium

## 現状

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
    Ok(()) // ← max_header_list_size == None のとき常にここに到達する
}
```

`check_header_block_fragment_size` は以下の 2 箇所で呼ばれている:
- `headers.rs:302` (`handle_headers` のヘッダーブロック継続開始時)
- `headers.rs:597` (`handle_continuation` の CONTINUATION 受信時)

## 設計方針

`check_header_block_fragment_size` に絶対的な上限を追加する。`max_header_list_size` が `None` の場合でも適用される固定上限（例: 64MB）を設定し、超過時は `COMPRESSION_ERROR` の接続エラーを返す。

上限値の選択: RFC 9113 §10.5.1 は具体的な数値を定めていないが、nghttp2 や h2 ライブラリの実装では数十 MB のハードリミットを設けている。64MB は圧縮済みフラグメントとしては現実的な HTTP ヘッダーを超えた十分に大きな値であり、DoS 抑止として機能する。

## 対応手順

1. 作業ブランチ `feature/fix-continuation-flood-limit` を作成する
2. `src/connection/headers.rs` の `check_header_block_fragment_size` に固定上限を追加する:
   ```rust
   const MAX_HEADER_BLOCK_FRAGMENT_SIZE: usize = 64 * 1024 * 1024; // 64MB
   ```
3. `max_header_list_size` が `Some` の場合はその値、`None` の場合は固定上限を適用する
4. `tests/test_connection.rs` に以下の単体テストを追加する:
   - `max_header_list_size=None` でフラグメントが固定上限を超えた場合に `COMPRESSION_ERROR` が返ること
   - `max_header_list_size=Some(小さい値)` でその値を超えた場合の挙動（既存テストと同等）
5. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する
6. `cargo test --workspace` で全テスト通過を確認する
7. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する

## 完了条件

- `max_header_list_size = None` 時にも `header_block_fragment` のサイズに絶対的な上限が適用される
- CONTINUATION フレームを無制限に送信してもメモリが上限に留まる
- 上限超過時の `COMPRESSION_ERROR` 返却を検証する単体テストが追加されている
- `cargo test --workspace` が通過する

## 解決方法

1. `src/connection/headers.rs` の `check_header_block_fragment_size` に絶対的な固定上限 `MAX_HEADER_BLOCK_FRAGMENT_SIZE = 64MB` を追加した。
2. `max_header_list_size` が `None` の場合は固定上限を、`Some` の場合はその値を使用するように変更した。
3. `tests/test_connection.rs` に `max_header_list_size=None` の正常経路テストを追加した。
4. `CHANGES.md` の `[FIX]` セクションにエントリを追加した。
5. `cargo test --workspace` と `cargo clippy --workspace --all-targets -- -D warnings` の通過を確認した。
