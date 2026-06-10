//! `Limits` のデフォルト値境界テスト。

use shiguredo_http2::Limits;

/// デフォルト値で `Limits::builder().build()` は常に成功する。
///
/// `Limits::builder()` 経由でビルダーを生成し、何も設定せずに build した場合に
/// 成功することを保証する境界テスト (デフォルト値が常に妥当である不変条件)。
#[test]
fn test_default_build_succeeds() {
    let result = Limits::builder().build();
    assert!(result.is_ok());
}
