# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-02

First public release.

### Added

- Interactive terminal interface built with ratatui: account list, details panel, live git and
  GitHub CLI status, keyboard navigation, overlays for help, forms and confirmations.
- Onboarding screen for a first run with no saved accounts, including a dependency check for `git`,
  the GitHub CLI and the OS credential store.
- Account management: add, rename, remove, re-authenticate and switch saved profiles.
- Account switching that updates `user.name`, `user.email`, the active `gh` account and the git
  credential helper, then verifies the result against `gh auth status`.
- Per-repository switching with `--local` or the <kbd>L</kbd> toggle.
- Detection of drift between the git identity and the authenticated GitHub CLI account.
- Token storage in the OS credential store (Keychain, Credential Manager, Secret Service), with a
  clear fallback when no store is available and no plaintext alternative.
- Redaction of anything shaped like a GitHub token in every message that can reach the terminal.
- Non-interactive commands: `list`, `current`, `switch`, `add`, `remove`, `rename`, `auth`,
  `doctor` and `version`, with `--json` output for `list` and `current`.
- `GITSWITCH_CONFIG_DIR` and `--config-dir` for overriding the configuration location.
- Versioned configuration file with migration from the pre-1 layout, atomic writes and owner-only
  permissions.
- Test suite covering the store, git and GitHub CLI integration, the service layer, the CLI and the
  rendered interface, with no dependency on a real GitHub account.

[Unreleased]: https://github.com/xRookieFight/gitswitch/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/xRookieFight/gitswitch/releases/tag/v0.1.0
