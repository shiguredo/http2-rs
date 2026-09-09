# CONNECT リクエストの :authority 検証が host 部 (uri-host) を検査しない

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-connect-authority-host-validation
- Polished: 2026-09-09

## 目的

HTTP/2 の CONNECT リクエストの `:authority` 検証 (`src/validation.rs` の `is_valid_connect_authority`) が、host 部 (uri-host) を一切検査せず、SP や制御文字を含む不正な値を許容する問題を修正する。RFC 9113 Section 8.5 / RFC 9112 Section 3.2.3 の authority-form の規定に整合させる。

## 現状

`is_valid_connect_authority` (`src/validation.rs`) は以下の検証のみを行う:

- 空文字でないこと
- IPv6 リテラルの場合: `[` で始まり `]` の後に `:` + ポートがあること (`[` と `]` の間は無検証)
- それ以外: 最後の `:` 以降が `0..=65535` の数字であること (`is_valid_port`)

host 部分 (uri-host。RFC 3986 Section 3.2.2 の IP-literal / IPv4address / reg-name) は無検証のため、`"foo bar:80"` や `"evil host:443"` のような SP 入りの値が `validate_field_value` (SP は値内部に許容) を通過し、`is_valid_connect_authority` も通過する。SP・制御文字に限らず、uri-host に許可されない `/`・`?`・`#`・`{`・`|`・`\`・`^`・`"`・`<`・`>` などの文字、非 ASCII バイト、不正な pct-encoded (`%zz`)、IPv6 リテラル内部の不正文字 (`[:: 1]:443`) も同様に通過する。

RFC 9112 Section 3.2.3 の `authority-form = uri-host ":" port` の uri-host は RFC 3986 Section 3.2.2 の host 文法に従い、`reg-name = *( unreserved / pct-encoded / sub-delims )` の文字集合 (ALPHA / DIGIT / `-` / `.` / `_` / `~` / `!` / `$` / `&` / `'` / `(` / `)` / `*` / `+` / `,` / `;` / `=` と pct-encoded の `%`) 以外を許さない。RFC 9113 Section 8.5 は CONNECT の `:authority` がこの authority-form と等価であることを要求する。

テスト (`tests/test_validation.rs`) にも SP 入り host のケースがなく、検証漏れを捕捉できない。ポート範囲外 (65535 超) と userinfo (`@`) は既存経路 (`is_valid_port` と `validate_request_headers` の `@` 拒否) で既に拒否されるため、回帰テストのみ必要である。

## 設計方針

- `is_valid_connect_authority` に host 部 (uri-host) の文字検査を追加し、RFC 3986 Section 3.2.2 の `reg-name` 文字集合 (ALPHA / DIGIT / `-` / `.` / `_` / `~` / `!` / `$` / `&` / `'` / `(` / `)` / `*` / `+` / `,` / `;` / `=`) と pct-encoded の `%` (後続 2 バイトが HEXDIG) 以外のバイトを拒否する。これにより SP・制御文字・非 ASCII バイト・その他の不正文字を一括で弾く
- IPv6 リテラルは `[` と `]` の間が空でなく、hex digit / `:` / `.` のみで構成されることを検査する (IPvFuture・zone ID は対象外)
- ポートは既存の `is_valid_port` (数字のみ、0..=65535) を維持する。userinfo (`@`) は既存の `validate_request_headers` の拒否経路で対応済みのため、回帰テストのみ追加する
- `tests/test_validation.rs` に、SP 入り host・`validate_field_value` を通過する制御文字 (0x01 / 0x7f / 値内部の HTAB) 入り host・uri-host に許可されない文字 (`/` など) 入り host・不正な IPv6 リテラル・ポート範囲外・userinfo の拒否テストと、正常な host の受理テストを追加する

## 完了条件

- `"foo bar:80"` 等の SP 入り host が `validate_request_headers` で拒否されること
- `validate_field_value` を通過する制御文字 (例: 0x01、0x7f、値内部の HTAB) 入り host が拒否されること
- uri-host に許可されない文字 (`/`, `?`, `#`, `{`, `|`, `\`, `^`, `"`, `<`, `>` など) を含む host が拒否されること
- `[:: 1]:443` 等、IPv6 リテラル内部に不正文字を含む host が拒否されること
- ポート範囲外 (65535 超) と userinfo (`@`) を含む host が拒否されること (既存挙動の回帰テスト)
- 正常な host (`example.com:443`、`[::1]:443` 等) が引き続き受理されること
- テストが追加され、`cargo test --all` が通過すること
