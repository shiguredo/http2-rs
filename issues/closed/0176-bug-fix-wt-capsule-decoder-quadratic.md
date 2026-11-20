# `CapsuleDecoder::decode` の先頭 drain による O(n²) デコードを解消する

- Created: 2026-11-16
- Completed: 2026-11-20
- Branch: feature/fix-wt-capsule-decoder-quadratic
- Polished: 2026-11-17

## 目的

`src/webtransport/capsule.rs` の `CapsuleDecoder::decode` は capsule を 1 個処理するたびに `Vec::drain(..total_len)` で残り全バイトを memmove する。最小 2 バイトの capsule を大量に並べると入力サイズの二乗で CPU を消費し、リモートから CPU DoS を成立させられるため、線形時間の実装に変更する。

## 現状

`CapsuleDecoder::decode` はバッファ先頭から capsule を読み、`total_len` 分を `drain` で削除する。このため 16 KiB の DATA に 2 バイト capsule を 8192 個入れるだけで約 67 MB の memmove が発生する。作成環境での一時計測では 16 KiB で約 1.2 ms、64 KiB で約 9.7 ms、256 KiB で約 128 ms であり、入力サイズに対して二次関数的に増加した（絶対値は環境依存）。

HTTP/2 の DATA フレームは小さくても、ピアは多数のフレームに分割して送れるため、この増幅率は実用的な攻撃面になる。

## 設計方針

- バッファの読み取り位置 (offset) を保持し、`decode` では読み取り位置を進めるだけにする。未処理バイトがなくなった時点でバッファをクリアする。offset 方式でバッファ長が上限に達したときは、未処理バイトを先頭に詰める（capsule ごとには詰めない）。1 capsule あたり残り全バイトの移動を行わないこと。
- または `VecDeque` を使い、先頭から必要分だけ pop する。`VecDeque` を使う場合も、`drain` や `make_contiguous` を capsule ごとに呼ばないこと。`VecDeque` 方式ならバッファ長は未処理バイト数になり、上限以下に保てる。
- `feed` のバッファ上限 (16 MiB) の判定はバッファ長を基準にする。offset 方式では `buffer.len() + data.len()` が上限を超える場合に処理済みの先頭バイトを詰めてから追加する。詰めた後に再度上限を判定し、なお超過する場合は `InvalidInput` を返す。加算オーバーフローを避けるため、現行と同じく `saturating_sub` で判定する。これにより、処理済みバイトが上限を圧迫して未処理データが上限未満でも `feed` が `InvalidInput` を返す状態にならない。offset は常に `buffer.len()` 以下であることを不変条件として検査する。
- `remaining()` は未処理バイト数を返す（`decode()` 後に減少し、`clear()` 後は 0）。`clear()` はバッファと読み取り位置の両方を初期化する。`remaining()` の doc も未処理バイト数に合わせて更新する。
- `decode` がエラーを返すときは読み取り位置を進めない（現行と同じく、エラーになった capsule のバイト列をバッファに残す）。
- 2 バイト capsule の大量入力を処理する性能回帰テストを `tests/test_webtransport/capsule.rs` に追加する。`std::time::Instant` を使った通常のテストとし、criterion は追加しない。16 KiB / 64 KiB / 256 KiB の 2 バイト capsule 入力をデコードし、256 KiB の処理時間が 16 KiB の 64 倍以下であることを検証する（線形なら 16 倍、現行の二次実装なら 100 倍以上）。各サイズを複数回計測して最小値を使うなど、CI の負荷変動で失敗しないよう余裕を持たせる。
- 分割 feed のテストを `tests/test_webtransport/capsule.rs` に追加する。capsule のバイト列を複数回の `feed` に分割し、途中の `decode` が `Ok(None)` を返し、最後の `feed` 後に capsule が正しくデコードされることを検証する。
- `feed` の上限判定が処理済みバイトを数えないことを検証するテストを `tests/test_webtransport/capsule.rs` に追加する。例えば `with_max_buffer_size` を小さく設定し、処理済み capsule の後ろに部分 capsule が残っている状態で `feed` が成功することを確認する。

## 完了条件

- `CapsuleDecoder::decode` が capsule 数に対して線形時間で動作すること。1 capsule あたり残り全バイトの移動を行わないこと。
- 16 KiB / 64 KiB / 256 KiB の 2 バイト capsule 入力で、処理時間が入力サイズにほぼ比例すること。性能回帰テストでは、256 KiB の処理時間が 16 KiB の 64 倍以下であることを検証する（線形なら 16 倍、現行の二次実装なら 100 倍以上）。
- 既存の capsule roundtrip / unknown / padding のテストが維持されていること。
- 分割 feed のテストが追加され、capsule のバイト列を複数回の `feed` に分割した場合でも正しくデコードされること。
- `remaining()` が `decode()` 後に未処理バイト数まで減少し、`clear()` 後に 0 になることをテストで検証していること。
- `feed` の上限判定が処理済みバイトを上限に数えないこと（処理済みの先頭バイトが残っていても、未処理データが上限未満なら追加の `feed` が成功すること）。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。

## 解決方法

- `CapsuleDecoder` の受信バッファを `VecDeque` に変更した。正常にデコードした先頭バイトだけを取り除き、後続データを移動しない。
- ヘッダーがリング境界をまたぐ場合は最大 16 バイトを一時領域にコピーする。ペイロードが境界をまたぐ場合は現在の Capsule のペイロードだけを連結する。部分入力やデコードエラーではバッファを消費しない。
- `feed` は未処理バイト数を基準に上限を判定する。分割入力では最大幅の Type と Length、消費済み領域の再利用、`remaining` と `clear` を検証した。
- 16 KiB・64 KiB・256 KiB のデコード時間を各 5 回測定した。回帰テストで 256 KiB が 16 KiB の 64 倍以下であることを確認した。
- フォーマット、Clippy、ワークスペーステストが通過した。
