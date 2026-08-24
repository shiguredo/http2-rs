# CONNECT リクエストの :authority 検証が host 部 (uri-host) を検査しない

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-connect-authority-host-validation
- Polished: {YYYY-MM-DD}

## 目的

HTTP/2 の CONNECT リクエストの `:authority` 検証 (`src/validation.rs` の `is_valid_connect_authority`) が、host 部 (uri-host) を一切検査せず、SP や制御文字を含む不正な値を許容する問題を修正する。RFC 9113 Section 8.5 / RFC 9112 Section 3.2.3 の authority-form の規定に整合させる。

## 現状

`is_valid_connect_authority` (`src/validation.rs`) は以下の検証のみを行う:

- 空文字でないこと
- IPv6 リテラルの場合: `[` で始まり `]` の後に `:` + ポートがあること
- それ以外: 最後の `:` 以降が `0..=65535` の数字であること (`is_valid_port`)

host 部分 (uri-host。RFC 3986 Section 3.2.2 の IP-literal / IPv4address / reg-name) は無検証のため、`"foo bar:80"` や `"evil host:443"` のような SP 入りの値が `validate_field_value` (SP は値内部に許容) を通過し、`is_valid_connect_authority` も通過する。

RFC 9112 Section 3.2.3 の `authority-form = uri-host ":" port` の uri-host は SP・制御文字等を許さない。RFC 9113 Section 8.5 は CONNECT の `:authority` がこの authority-form と等価であることを要求する。

テスト (`tests/test_validation.rs`) にも SP 入り host のケースがなく、検証漏れを捕捉できない。

## 設計方針

- `is_valid_connect_authority` に host 部の文字検査を追加し、SP・制御文字 (0x00-0x20, 0x7f) を含む値を拒否する
- ポート範囲外 (65535 超)、userinfo (`@` を含む) のケースも必要に応じて明示的に検査・テストする
- `tests/test_validation.rs` に SP 入り host・制御文字入り host・ポート範囲外のテストを追加する

## 完了条件

- `"foo bar:80"` 等の SP 入り host が `validate_request_headers` で拒否されること
- 制御文字入り host が拒否されること
- 正常な host (`example.com:443`、`[::1]:443` 等) が引き続き受理されること
- テストが追加され、`cargo test --all` が通過すること
