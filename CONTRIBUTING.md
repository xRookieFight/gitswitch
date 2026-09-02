# Contributing to gitswitch

Thanks for taking the time to help. Bug reports, documentation fixes and features that sharpen the
core purpose - switching GitHub accounts quickly and safely - are all welcome.

## Getting started

```bash
git clone https://github.com/xRookieFight/gitswitch
cd gitswitch
cargo test
cargo run
```

You need Rust 1.88 or newer (edition 2024). `git` and the GitHub CLI are useful for manual testing
but are not required to build or to run the test suite.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

CI runs the same checks on Linux, macOS and Windows, plus `cargo audit`.

## Ground rules for changes

- **Never write a secret to disk, a log or the screen.** Tokens belong in the OS credential store and
  are piped to `gh` on stdin. Anything that reaches the terminal goes through
  `gitswitch::process::redact` first.
- **Tests must not need a real GitHub account.** Use `gitswitch::testing::MockRunner` for
  subprocesses and `gitswitch::secrets::memory::MemoryStore` for credentials. A test that requires
  network access will not be merged.
- **Keep the layers separate.** `service.rs` holds behaviour shared by the CLI and the TUI;
  `tui/app.rs` holds state and key handling; `tui/ui.rs` is a pure function from state to frame. New
  behaviour usually belongs in the service layer, with a thin call from both front ends.
- **Errors must be actionable.** Add a variant to `error::Error` and, where a next step exists, a
  matching arm in `Error::hint`.
- **No new dependency without a reason.** It must be actively maintained and MIT/Apache-2.0
  compatible.

## Adding a screen or a shortcut

1. Add the state to `tui/app.rs` and handle the key in the matching `on_*_key` function.
2. Render it in `tui/ui.rs`.
3. Cover the key handling in `tui/app.rs` tests and the rendering in `tests/tui.rs`.
4. Document the shortcut in the in-app help (`draw_help`) and in the README table.

## Changing the on-disk format

Bump `store::SCHEMA_VERSION`, add the migration step to `store::migrate` and add a test that loads a
document written by the previous version. Old configurations must keep working.

## Commit messages

Conventional commit titles, one purpose per commit:

```
feat: switch the credential helper along with the account
fix: treat a logged-out gh as an empty account list
docs: document GITSWITCH_CONFIG_DIR
```

## Reporting bugs

Open an issue with the template. Include your OS, `gitswitch version`, `gh --version` and the output
of `gitswitch doctor`. Never paste a token - redact it even if you think it is expired.
