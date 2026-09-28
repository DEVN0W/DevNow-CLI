<p align="center">
  <img src="assets/devnow-cli-branding-preview.png" alt="DevNow Agent" width="736" />
</p>

<p align="center">
  An open-source, terminal-first AI software engineering agent with single-agent and multi-agent workflows.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache 2.0" src="https://img.shields.io/badge/license-Apache--2.0-7fc8f8.svg" /></a>
  <a href="https://github.com/DEVN0W/DevNow-Agent/issues"><img alt="GitHub issues" src="https://img.shields.io/github/issues/DEVN0W/DevNow-Agent" /></a>
  <a href="https://github.com/DEVN0W/DevNow-Agent/pulls"><img alt="Pull requests welcome" src="https://img.shields.io/badge/PRs-welcome-7fc8f8.svg" /></a>
</p>

DevNow Agent works inside your local project, can edit and run code, and supports three operating modes: **Single AI**, **Multi-Agent**, and **Multi-Agent Strategic**. It is designed for developers who want a configurable agent experience that can use different model providers and specialized agent teams.

![DevNow Agent terminal preview](assets/devnow-classic-hero-cli-preview.png)

## Highlights

- A branded terminal UI built for day-to-day development work
- Single-agent mode for focused tasks
- Manual multi-agent delegation with `@agent-name`
- Strategic multi-agent workflows with configurable routing and review loops
- Configurable model providers, models, sandboxing, and approval policies
- MCP server support and non-interactive execution
- Open-source under the Apache License 2.0

## Requirements

- Git 2.23 or newer
- Rust stable, including Cargo, `rustfmt`, and Clippy
- 4 GB RAM minimum; 8 GB recommended
- macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11

On Windows, PowerShell can build the project natively. WSL2 is recommended when you want a Linux development environment.

## Install from source

### 1. Clone the project

```bash
git clone --branch master https://github.com/DEVN0W/DevNow-Agent.git
cd DevNow-Agent/devnow-rs
```

### 2. Install Rust

Install Rust with [rustup](https://rustup.rs/), then add the required components:

```bash
rustup component add rustfmt clippy
```

### 3. Build a release binary

```bash
cargo build --release --bin codex
```

The Rust workspace currently produces an internal binary named `codex`. Install that artifact under the public `devnow` command:

macOS or Linux:

```bash
mkdir -p "$HOME/.local/bin"
cp target/release/codex "$HOME/.local/bin/devnow"
chmod +x "$HOME/.local/bin/devnow"
export PATH="$HOME/.local/bin:$PATH"
```

Windows PowerShell:

```powershell
$devNowBin = "$env:LOCALAPPDATA\DevNow\bin"
New-Item -ItemType Directory -Force $devNowBin | Out-Null
Copy-Item .\target\release\codex.exe "$devNowBin\devnow.exe"
$env:Path = "$devNowBin;$env:Path"
```

Add the chosen binary directory to your shell profile or user `PATH` to keep the `devnow` command available after restarting the terminal.

### 4. Create a DevNow configuration directory

DevNow supports a custom configuration home through `CODEX_HOME`. Set it to a DevNow-owned directory so your settings stay separate from other tools.

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

Persist `CODEX_HOME` in your shell profile or operating-system environment variables if you want to use the same location in future sessions.

### 5. Configure a model provider

Create `$CODEX_HOME/config.toml`. Never commit real API keys; store credentials in environment variables.

Basic configuration:

```toml
model = "YOUR_MODEL_NAME"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
```

For an OpenAI-compatible endpoint, add a provider:

```toml
model = "YOUR_MODEL_NAME"
model_provider = "my-provider"
approval_policy = "on-request"
sandbox_mode = "workspace-write"

[model_providers.my-provider]
name = "My Provider"
base_url = "https://YOUR-ENDPOINT.example/v1"
env_key = "DEVNOW_API_KEY"
wire_api = "responses"
```

Then set the credential in your terminal:

```bash
export DEVNOW_API_KEY="your-api-key"
```

Windows PowerShell:

```powershell
$env:DEVNOW_API_KEY = "your-api-key"
```

Azure OpenAI example:

```toml
model = "YOUR_AZURE_DEPLOYMENT_NAME"
model_provider = "azure-openai"

[model_providers.azure-openai]
name = "Azure OpenAI"
base_url = "https://YOUR-RESOURCE.openai.azure.com/openai"
env_key = "AZURE_OPENAI_API_KEY"
wire_api = "responses"
query_params = { api-version = "YOUR_SUPPORTED_API_VERSION" }
```

Set `AZURE_OPENAI_API_KEY` in your environment before starting DevNow. Model names, endpoints, and API versions must match the provider account you use.

### 6. Start DevNow

Open the project you want the agent to work on and run:

```bash
cd /path/to/your/project
devnow
```

Or start with an initial request:

```bash
devnow "explain this project and suggest the first improvement"
```

## Configure operating modes

Use `/mode` inside the terminal UI to select a workflow. `/plan` remains available for planning within any mode.

### Single AI

Single AI is the default. It uses the model and provider from `$CODEX_HOME/config.toml`; no additional roster file is required.

### Multi-Agent

Multi-Agent gives you manual control over a team. Copy the included sample roster:

macOS or Linux:

```bash
cp ../docs/agentic.sample.toml "$CODEX_HOME/agentic.toml"
```

Windows PowerShell, when run from `devnow-rs`:

```powershell
Copy-Item ..\docs\agentic.sample.toml "$env:CODEX_HOME\agentic.toml"
```

Edit each agent's `model` and `model_provider` to match your configuration. Select **Multi-Agent** with `/mode`, then address an agent at the beginning of a prompt:

```text
@developer implement the requested API and add focused tests
```

Multiple leading mentions can dispatch independent assignments:

```text
@principal-architect @developer review the design and implementation in parallel
```

### Multi-Agent Strategic

Strategic mode runs an automatic workflow containing roles such as architect, planner, developer, critic, security, quality, execution, and judge.

Copy the strategic sample:

macOS or Linux:

```bash
cp ../docs/multi-agent.sample.toml "$CODEX_HOME/multi_agent.toml"
```

Windows PowerShell, when run from `devnow-rs`:

```powershell
Copy-Item ..\docs\multi-agent.sample.toml "$env:CODEX_HOME\multi_agent.toml"
```

Update the model/provider values, restart DevNow, and choose **Multi-Agent Strategic** from `/mode`.

## Screenshots

### Terminal experience

![DevNow Agent terminal experience](assets/devnow-classic-hero-cli-preview.png)

### Brand system

![DevNow Agent pixel helmet concepts](assets/devnow-helmet-concepts.png)

## Development

From `devnow-rs`:

```bash
cargo test -p codex-tui
cargo test -p codex-tui-app-server
just fmt
just argument-comment-lint
```

See [Contributing](docs/contributing.md) for the complete workflow. User configuration examples are available in [docs/config.md](docs/config.md), [docs/agentic.sample.toml](docs/agentic.sample.toml), and [docs/multi-agent.sample.toml](docs/multi-agent.sample.toml).

## Open-source project

Bug reports, feature proposals, documentation improvements, and pull requests are welcome. Please read the [contribution guide](docs/contributing.md) and [security policy](SECURITY.md) before participating.

DevNow Agent is licensed under the [Apache License 2.0](LICENSE). This project is derived from open-source software; required copyright and third-party attribution notices are retained in [NOTICE](NOTICE).
