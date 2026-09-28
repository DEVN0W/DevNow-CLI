# Contributing to DevNow Agent

Thanks for helping improve DevNow Agent. Bug reports, feature proposals, documentation changes, tests, and code contributions are welcome.

## Before you start

1. Search the [issues](https://github.com/DEVN0W/DevNow-Agent/issues) to avoid duplicates.
2. For a significant feature or architecture change, open an issue before investing in an implementation.
3. Keep each pull request focused on one problem.
4. Never include credentials, tokens, private prompts, customer data, or generated local configuration.

## Development workflow

1. Fork the repository and clone your fork.
2. Create a topic branch from `master`, such as `feat/better-agent-routing` or `fix/windows-startup`.
3. Make the smallest complete change that solves the issue.
4. Add or update tests for behavior changes.
5. Update user documentation and snapshots when output or the terminal UI changes.
6. Run the checks that apply to the crates you changed.
7. Open a pull request describing what changed, why it changed, and how it was tested.

## Rust checks

Run commands from `devnow-rs`:

```bash
just fmt
just fix -p <crate-you-changed>
just argument-comment-lint
cargo test -p <crate-you-changed>
```

For TUI changes, run the relevant snapshot suite and review every new snapshot before accepting it:

```bash
cargo test -p codex-tui
cargo insta pending-snapshots -p codex-tui
```

If the parallel app-server TUI implements the same behavior, update and test it too.

## Pull-request expectations

- Explain the user-visible result.
- Link the related issue when one exists.
- Include focused tests and documentation.
- Keep commits understandable and free of unrelated formatting churn.
- Preserve the Apache 2.0 license and all applicable notices.
- Confirm that no secrets or sensitive data are included.

Maintainers may request revisions for correctness, security, scope, test coverage, documentation, or long-term maintainability.

## Reporting security issues

Do not disclose a suspected vulnerability in a public issue. Follow the private process in the [security policy](../SECURITY.md).
