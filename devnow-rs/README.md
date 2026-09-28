# DevNow Agent Rust workspace

This directory contains the native DevNow Agent implementation. The project is derived from OpenAI Codex under the Apache License 2.0; required attribution is retained in the repository root.

Some crate names, environment variables, protocol types, and the build artifact still use `codex-*` or `CODEX_*` identifiers for source and wire compatibility. Public documentation and installed commands use the DevNow Agent name.

## Main crates

- `cli/` provides the top-level command-line entry point.
- `tui/` provides the interactive terminal interface.
- `tui_app_server/` mirrors TUI behavior for the app-server runtime.
- `exec/` provides non-interactive execution.
- `core/` contains the inherited runtime and agent logic.
- `protocol/` and `app-server-protocol/` define internal API types.

## Development

Run commands from this directory. Follow the repository [contribution guide](../docs/contributing.md), including focused crate tests, formatting, lint checks, and snapshot review for user-visible TUI changes.

For installation, provider setup, and operating modes, start with the root [README](../README.md).
