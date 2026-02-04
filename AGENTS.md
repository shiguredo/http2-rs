# AGENTS

- Premature Optimization is the Root of All Evil
- 一切忖度しないこと
- 常に日本語を利用すること
- 全角と半角の間には半角スペースを入れること
- 絵文字を使わないこと
- RFC 準拠を最優先すること

## レビューについて

- レビューはかなり厳しくすること
- レビューの表現は、シンプルにすること
- レビューの表現は、日本語で行うこと
- レビューの表現は、指摘内容を明確にすること
- レビューの表現は、指摘内容を具体的にすること
- レビューの表現は、指摘内容を優先順位をつけること
- レビューの表現は、指摘内容を優先順位をつけて、重要なものから順に記載すること
- ドキュメントは別に書いているので、ドキュメトに付いては考慮しないこと
- 変更点とリリースノートの整合性を確認すること

## コミットについて

- 勝手にコミットしないこと
- コミットメッセージは確認すること
- コミットメッセージは日本語で書くこと
- コミットメッセージは命令形で書くこと
- コミットメッセージは〜するという形で書くこと

## サンプルについて

- サンプルは **お手本** なので性能と堅牢性を両立させること
- サンプルは RFC に準拠していること

## RFC について

- WebSocket over HTTP/2 は実装しないこと
- RFC 7540 と 8740 は廃止されて RFC 9113 になってる
- 非推奨 (deprecated) 機能は実装しないこと
- 廃止 (obsolete) 機能は実装しないこと
- 主要ブラウザがサポートを削除した機能は実装しないこと
  - サーバープッシュ (PUSH_PROMISE) は Chrome/Firefox/Safari が削除済み

## テストについて

- pbt 以下に unittest を書かないこと
- unittest は pbt で実現できないものだけを書くこと
- pbt は prop_ というファイル名、関数名にすること

## pre-commit

- make fmt / make clippy / make check / make test を実行すること

## Rust

- 性能より堅牢性を優先すること
- PBT(Property-Based Testing) や Fuzzing で必ずテストを行うこと

### rustls

- aws-lc-rs を使うこと
  - aws-lc-rs の最新は 1.15
- webpki-roots を使わず rustls-platform-verifier を使うこと
  - rustls-platform-verifier の最新は 0.6
- rustls-pemfile を使わず rustls-pki-types を使うこと
- rustls-pki-types の最新は 1.14
- rcgen の最新は 0.14
