# `examples/wt_server` の証明書キャッシュが秘密鍵を既定権限で一時ディレクトリに保存する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-server-key-cache
- Polished: 2026-11-17

## 目的

`examples/wt_server` は自己署名証明書のキャッシュを `std::env::temp_dir()` 配下の固定名 `wt-server-http2-cert.jsonc` に保存する。このファイルには base64 化した秘密鍵が含まれる。Unix の共有 `/tmp` では umask 依存 (通常 0644) で他ユーザーから読め、固定名のシンボリックリンクを事前に置かれると書き込み先を追従する。`load_cached_cert` の読み込みもシンボリックリンクを追従する。キャッシュの生成・読み込みを安全にする。

## 現状

`examples/wt_server/src/tls.rs` の `generate_and_cache_cert` は `std::fs::write(&path, &jsonc)` で証明書と秘密鍵を書き込む。`cache_path()` は `std::env::temp_dir().join(CACHE_FILENAME)` を返す。`std::fs::write` は `File::create` 相当で、モードは umask 依存 (通常 0644)、既存のシンボリックリンクを追従する。

`load_cached_cert` は `std::fs::read_to_string` でキャッシュを読むため、読み込みでもシンボリックリンクを追従する。ファイルの所有者・パーミッション・証明書と鍵の対応・X.509 の `notAfter` を検証せず、JSONC の `created_at + CERT_VALIDITY_DAYS * 24 * 3600` で期限を判定している。

`generate_tls_server` はキャッシュを `TlsServerConfig::from_der` に渡す。`TlsServerConfig::from_der` は rustls の `with_single_cert` を呼び、証明書と秘密鍵の対応は検証されるが、不一致の場合はエラーになって起動が失敗する。キャッシュミスとして再生成する経路はない。

interop は `interop/browser/index.html` の `serverCertificateHashes` でこの証明書をピン留めする。`interop/browser/run.mjs` は起動ログからハッシュを取得するため、キャッシュが再生成されても interop 自体は追随するが、`serverCertificateHashes` を固定している利用者はハッシュの再設定が必要になる。

## 設計方針

- キャッシュディレクトリは、Unix で `XDG_RUNTIME_DIR` が設定されていればその配下、未設定なら `std::env::temp_dir()` 配下の `wt-server-<uid>` とする。`uid` は `libc::geteuid()` で取得する。Windows では `std::env::temp_dir()` 配下の `wt-server` とし、ディレクトリを作成する (パーミッション検証はしない)。
- キャッシュディレクトリは Unix では `0700` で作成する。既に存在する場合は、シンボリックリンクでないこと・ディレクトリであること・所有者が自分であること・`0700` であることを検証し、満たさなければ起動エラーにする。
- Unix では一時ファイルを `OpenOptions` の `create_new` と `mode(0o600)` で作成し、JSONC を書いてから `rename` でキャッシュパスへ置き換える。Windows では `mode` 指定なしで同じ `create_new` + `rename` を使う。`create_new` は既存のシンボリックリンクを拒否し、`rename` はシンボリックリンクを追従せず置き換える。一時ファイル名は PID などを含めて一意にする。書き込みに失敗したら一時ファイルを削除する。
- 読み込み時は `symlink_metadata` でキャッシュパスがシンボリックリンクでないことと通常ファイルであることを確認する。Unix では所有者が `libc::geteuid()` と一致し、`mode & 0o077 == 0` であることを検証する。Windows では所有者・パーミッションの検証をスキップする (`#[cfg(unix)]`)。
- 証明書と鍵の対応は `rustls::sign::CertifiedKey::from_der` で検証する。X.509 の `notBefore` が現在以前であり、`notAfter` が現在から `MIN_REMAINING_HOURS` 以上先であることを `x509-parser` で検証する。期限判定は `created_at` ではなく `notBefore` / `notAfter` を正とする。
- シンボリックリンク・通常ファイルでない場合、または所有者・パーミッション・証明書と鍵の対応・有効期限のいずれかの検証に失敗した場合は、キャッシュミスとして扱い、新規生成して置き換える (置き換えに失敗した場合は起動エラーにする)。検証失敗の理由は `tracing::info!` でログ出力する。
- 秘密鍵はキャッシュに含める。含めないと再起動ごとに証明書が変わり、`serverCertificateHashes` で使う SHA-256 ハッシュを維持できないため、秘密鍵をキャッシュしない運用は採用しない。
- キャッシュパスが変わるため、旧パスの `std::env::temp_dir()/wt-server-http2-cert.jsonc` は読み込まない。README に旧パスのキャッシュは使われないことと、最初の起動でハッシュが変わることを書く。
- テストのために、キャッシュの読み書き関数はキャッシュパス (またはキャッシュディレクトリ) を引数で受け取る形にし、`generate_tls_server` は既定のパスを渡す。テストは `tls.rs` の `#[cfg(test)]` に置き、リポジトリ外の一時ディレクトリを使って実行する。
- `examples/wt_server/README.md` にキャッシュのパス・パーミッション・検証挙動・削除方法を書く。`tls.rs` のモジュール doc のキャッシュパス説明も更新する。

## 変更対象

- `examples/wt_server/src/tls.rs`: キャッシュディレクトリの決定と作成、一時ファイル + `rename` による書き込み、読み込み時の検証、テスト
- `examples/wt_server/Cargo.toml`: Unix の所有者検証用 `libc`、X.509 の `notBefore` / `notAfter` 解析用 `x509-parser`
- `examples/wt_server/README.md`: キャッシュのパス・パーミッション・検証挙動・削除方法

## 完了条件

- キャッシュディレクトリが Unix で `0700`、キャッシュファイルが Unix で `0600` で作成されること。
- 書き込み・読み込みの両方でシンボリックリンクを追従しないこと。読み込み時にシンボリックリンクならキャッシュミスとして再生成すること。
- 読み込み時に Unix の所有者とパーミッション、証明書と鍵の対応、X.509 の `notBefore` / `notAfter` を検証すること。検証に失敗した場合はキャッシュミスとして再生成すること。
- 新しいキャッシュパスに 0644 のファイルが存在する場合は読み込み時に検証で弾かれ、0600 で再生成されること。
- `examples/wt_server/README.md` のキャッシュ節と `tls.rs` のモジュール doc が新しいパス・パーミッション・検証挙動・削除方法・旧パスの扱いに更新されていること。
- キャッシュの安全性を検証するテストが追加されていること。少なくとも、作成時のパーミッション、シンボリックリンクの拒否、所有者・パーミッション・証明書と鍵の対応・有効期限の検証、有効なキャッシュの再利用を確認すること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
