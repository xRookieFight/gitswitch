# Security Policy

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | yes |

Security fixes land on the latest release. Please upgrade before reporting.

## Reporting a vulnerability

Report privately through GitHub's
[security advisory form](https://github.com/xRookieFight/gitswitch/security/advisories/new).
Please do not open a public issue for a security problem.

Include the version, your platform, what you observed and how to reproduce it. **Never include a
real token**, even an expired one.

You can expect an acknowledgement within 7 days and an assessment within 30 days. Fixed issues are
published as a GitHub Security Advisory with credit to the reporter unless you prefer otherwise.

## What gitswitch does with your credentials

- **Passwords are never requested, handled or stored.**
- The only secret handled is a GitHub personal access token, and only when you supply one.
- Tokens are stored in the operating system credential store (Keychain on macOS, Credential Manager
  on Windows, Secret Service on Linux) under the service name `gitswitch`. When no credential store
  is reachable, gitswitch reports it and does not cache the token - there is no plaintext fallback.
- Tokens are passed to `gh` on stdin, never as command-line arguments, so they never appear in the
  process list or in shell history.
- `accounts.json` holds only profile names, GitHub logins, git identities, hosts and a boolean
  saying whether a token is cached. It is created with `0600` permissions inside a `0700` directory
  on Unix and written atomically.
- Every string captured from a subprocess passes through `process::redact`, which replaces anything
  matching a GitHub token shape (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`) with
  `[redacted]` before it can reach the terminal or an error message.
- gitswitch makes no network requests of its own. All GitHub communication goes through the official
  GitHub CLI.

## Scope

In scope: token disclosure, credential storage weaknesses, incorrect file permissions, command
injection, an account switch that silently applies the wrong identity.

Out of scope: vulnerabilities in `git` or the GitHub CLI themselves (report those upstream), and
issues that require an attacker who already has local access to your unlocked user session.
