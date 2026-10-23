# DevoNow Agent

DevoNow Agent is an open-source, terminal-first software engineering agent. It can inspect projects, edit files, run commands, and coordinate specialized agents while letting you choose the model provider that fits your environment.

## Highlights

- Focused single-agent workflows
- Manual delegation with `@agent-name`
- Strategic multi-agent planning, implementation, review, and validation
- Support for Claude, OpenAI-compatible APIs, Azure AI Foundry, Ollama, and LM Studio
- Interactive terminal UI and non-interactive execution
- Configurable approval and sandbox policies
- Model Context Protocol (MCP) support
- Apache 2.0 license

## Requirements

- Git 2.23 or newer
- Rust stable with Cargo, rustfmt, and Clippy
- 4 GB RAM minimum; 8 GB recommended
- macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11

## Install from source

Clone the repository and enter the Rust workspace:

```bash
git clone https://github.com/DEVN0W/DevNow-Agent.git
cd DevNow-Agent/devnow-rs
```

Install the required Rust components and build the release binary:

```bash
rustup component add rustfmt clippy
cargo build --release --bin codex
```

The workspace retains the internal `codex` binary name for upstream compatibility. Install it as the public `devnow` command.

macOS or Linux:

```bash
mkdir -p "$HOME/.local/bin"
cp target/release/codex "$HOME/.local/bin/devnow"
chmod +x "$HOME/.local/bin/devnow"
```

Windows PowerShell:

```powershell
$devoNowBin = "$env:LOCALAPPDATA\DevoNow\bin"
New-Item -ItemType Directory -Force $devoNowBin | Out-Null
Copy-Item .\target\release\codex.exe "$devoNowBin\devnow.exe"
```

Add the installation directory to your `PATH`, then verify the command:

```bash
devnow --version
```

## Configure DevoNow

DevoNow reads configuration from `CODEX_HOME`. The variable name is retained for compatibility with the upstream codebase.

macOS or Linux:

```bash
export CODEX_HOME="$HOME/.devnow"
mkdir -p "$CODEX_HOME"
```

Windows PowerShell:

```powershell
$env:CODEX_HOME = "$HOME\.devnow"
New-Item -ItemType Directory -Force $env:CODEX_HOME | Out-Null
```

Create `$CODEX_HOME/config.toml`:

```toml
model = "YOUR_MODEL_NAME"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
```

Provider credentials are read from environment variables and must not be committed to the repository. See [configuration](docs/config.md), [authentication](docs/authentication.md), and the [example configuration](docs/example-config.md) for provider-specific setup.

## Run

Open the project you want DevoNow to work on:

```bash
cd /path/to/your/project
devnow
```

You can also supply an initial request:

```bash
devnow "explain this project and suggest the first improvement"
```

For non-interactive use:

```bash
devnow exec "review this change and report blocking issues"
```

## Operating modes

Use `/mode` in the terminal UI to select a workflow.

### Single AI

Runs one agent with the model and provider configured in `config.toml`.

### Multi-Agent

Lets you delegate directly to configured specialists:

```text
@developer implement the requested API and add focused tests
```

Start with [docs/agentic.sample.toml](docs/agentic.sample.toml).

### Multi-Agent Strategic

Runs an orchestrated workflow across planning, implementation, review, quality, execution, and final judgment roles. Start with [docs/multi-agent.sample.toml](docs/multi-agent.sample.toml).

## Documentation

- [Getting started](docs/getting-started.md)
- [Installation](docs/install.md)
- [Configuration](docs/config.md)
- [Authentication](docs/authentication.md)
- [Sandbox modes](docs/sandbox.md)
- [Execution policies](docs/execpolicy.md)
- [Slash commands](docs/slash_commands.md)
- [Agent instructions](docs/agents_md.md)

## Development

Run commands from `devnow-rs`:

```bash
cargo test -p codex-tui
cargo test -p codex-tui-app-server
just fmt
just argument-comment-lint
```

See [CONTRIBUTING](docs/contributing.md) for the complete development workflow.

## Contributing

Issues and pull requests are welcome. Please read the [contribution guide](docs/contributing.md), [Code of Conduct](CODE_OF_CONDUCT.md), and [support guide](SUPPORT.md) before participating.

## License and attribution

DevoNow Agent is licensed under the [Apache License 2.0](LICENSE). It is derived from the open-source [OpenAI Codex](https://github.com/openai/codex) codebase and is not an official OpenAI product. Required copyright, license, and third-party attribution notices are retained in [NOTICE](NOTICE).
