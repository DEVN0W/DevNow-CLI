# Configuration

Start with the provider walkthrough in the root [README](../README.md#configure-a-model-provider). The generated schema for every supported setting is available at `devnow-rs/core/config.schema.json`.

## Operating modes

Use `/mode` to select one of DevNow's three operating modes:

- **Single AI** is the startup default and uses the model, provider, and runtime settings from `config.toml`.
- **Multi-Agent** is manual orchestration. It loads `CODEX_HOME/agentic.toml`, but agents remain inactive until the root AI calls an agent tool such as `spawn_agent`.
- In Multi-Agent mode, begin a prompt with `@<agent-name>` to explicitly dispatch that configured agent. Use consecutive leading mentions such as `@frontend-dev @backend-dev` to run independent assignments from the same plan concurrently; each result is labeled by agent name. Sequential calls retain a bounded handoff history, so the next agent receives the prior request, result, and target workspace. When the task contains a new absolute target path, that project replaces the carried workspace and supplies its project instructions instead of inheriting the launch repository. The running-agent card shows each resolved roster model.
- **Multi-Agent Strategic** is automatic orchestration. It loads `CODEX_HOME/multi_agent.toml` and runs the configured strategic workflow for each turn.

Plan remains available through `/plan`; it is not a fourth `/mode` choice. `/collab` remains an alias for `/mode`.

## Model providers

If you do not set a custom `model_provider`, DevNow uses the built-in `openai`
provider. That is the default vanilla path.

To use Azure OpenAI or another compatible backend, add a custom provider under
`[model_providers.*]` in `$CODEX_HOME/config.toml`, then point your profile at it
with `model_provider`.

Minimal examples:

```toml
# 1) Vanilla default: no `model_provider` and no custom provider block.

[profiles.default]
# model_provider is omitted, so DevNow uses the built-in `openai` provider.
```

```toml
# 2) Azure OpenAI

[model_providers.azure-openai]
name = "Azure OpenAI"
base_url = "https://<your-resource>.openai.azure.com/openai"
env_key = "AZURE_OPENAI_API_KEY"
wire_api = "responses"
query_params = { api-version = "2025-04-01-preview" }

[profiles.default]
model_provider = "azure-openai"
```

```toml
# 3) Azure Foundry / Azure AI Projects

[model_providers.azure-foundry]
name = "Azure Foundry"
base_url = "https://<your-foundry-project-endpoint>/api/projects/<project-name>/openai/v1"
env_key = "AZURE_FOUNDRY_API_KEY"
wire_api = "responses"
extra_body = { agent_reference = { name = "emea-swd-multi-agent-ai", version = "2", type = "agent_reference" } }

[profiles.default]
model_provider = "azure-foundry"
```

DevNow merges `extra_body` into the request body only for the provider that
declares it, so you can keep OpenAI, Azure OpenAI, and Foundry side by side
without changing the normal path for other agents. For Foundry, use the
project endpoint plus `/openai/v1` so the final request path matches the
Azure AI Projects OpenAI client behavior.

The manual roster in `CODEX_HOME/agentic.toml` and strategic roster in
`CODEX_HOME/multi_agent.toml` can point each agent at one of those provider ids
with `model_provider`, so different agents can use different backends.

### Where to edit each file

- `$CODEX_HOME/config.toml`
  - Define the provider entries under `[model_providers.<id>]`.
  - Put each API base URL, token env var, and wire API there.
  - Use `extra_body` for provider-specific request payload fields such as
    Foundry `agent_reference`.
- `$CODEX_HOME/agentic.toml`
  - Define agents available for manual delegation.
  - Do not put provider URLs or API keys in this file.
  - Copy `docs/agentic.sample.toml` to use the recommended initial roster.
- `$CODEX_HOME/multi_agent.toml`
  - Define the automatic strategic roster and routing.
  - Set `model_provider = "<id>"` per agent to choose which provider that agent uses.

The built-in provider ids used by the recommended manual roster are `openai`
for GPT models and `claude` for managed Claude models. They reuse the effective
OpenAI configuration and the existing `DEVNOW_CLAUDE_API_KEY` with optional
`DEVNOW_CLAUDE_BASE_URL`; `agentic.toml` never duplicates those values.

Example:

```toml
# $CODEX_HOME/config.toml

[model_providers.azure-foundry-1]
name = "Azure Foundry - Architect"
base_url = "https://foundry-agent-1.example/api/projects/<project-name>/openai/v1"
env_key = "AZURE_FOUNDRY_AGENT_1_KEY"
wire_api = "responses"

[model_providers.openai]
name = "OpenAI"
wire_api = "responses"

[model_providers.azure-foundry-2]
name = "Azure Foundry - Quality"
base_url = "https://foundry-agent-2.example/api/projects/<project-name>/openai/v1"
env_key = "AZURE_FOUNDRY_AGENT_2_KEY"
wire_api = "responses"
```

```toml
# $CODEX_HOME/multi_agent.toml

default_agent = "architect"

[[agents]]
name = "architect"
role = "architect"
model = "gpt-5.5"
model_provider = "azure-foundry-1"
weight = 40
instructions = "Create the design only."

[[agents]]
name = "developer"
role = "developer"
model = "gpt-5.4"
model_provider = "openai"
weight = 40
instructions = "Implement the code."

[[agents]]
name = "quality"
role = "quality ai"
model = "gpt-4.1"
model_provider = "azure-foundry-2"
weight = 20
instructions = "Review code quality only."
```

## Connecting to MCP servers

DevNow can connect to MCP servers configured in `$CODEX_HOME/config.toml`. Consult the generated schema at `devnow-rs/core/config.schema.json` for supported MCP fields.

## MCP tool approvals

DevNow stores per-tool approval overrides for custom MCP servers under
`mcp_servers` in `$CODEX_HOME/config.toml`:

```toml
[mcp_servers.docs.tools.search]
approval_mode = "approve"
```

## Apps (Connectors)

Use `$` in the composer to insert a ChatGPT connector; the popover lists accessible
apps. The `/apps` command lists available and installed apps. Connected apps appear first
and are labeled as connected; others are marked as can be installed.

## Notify

DevNow can run a notification hook when the agent finishes a turn. Consult the generated configuration schema for the available notification fields.

When DevNow knows which client started the turn, the legacy notify JSON payload also includes a top-level `client` field. The TUI currently reports the compatibility identifier `codex-tui`, and the app server reports the `clientInfo.name` value from `initialize`.

## JSON Schema

The generated JSON Schema for `config.toml` lives at `devnow-rs/core/config.schema.json`.

## SQLite State DB

DevNow stores the SQLite-backed state DB under `sqlite_home` (config key) or the
`CODEX_SQLITE_HOME` environment variable. When unset, WorkspaceWrite sandbox
sessions default to a temp directory; other modes default to `CODEX_HOME`.

## Custom CA Certificates

DevNow can trust a custom root CA bundle for outbound HTTPS and secure websocket
connections when enterprise proxies or gateways intercept TLS. This applies to
login flows and to DevNow's other external connections, including components
that build reqwest clients or secure websocket clients through the inherited
`codex-client` CA-loading path and remote MCP connections that use it.

Set `CODEX_CA_CERTIFICATE` to the path of a PEM file containing one or more
certificate blocks to use a DevNow-specific CA bundle. This inherited variable
name is retained for compatibility. If `CODEX_CA_CERTIFICATE` is unset, DevNow
falls back to `SSL_CERT_FILE`. If neither variable is set, DevNow uses the
system root certificates.

`CODEX_CA_CERTIFICATE` takes precedence over `SSL_CERT_FILE`. Empty values are
treated as unset.

The PEM file may contain multiple certificates. DevNow also tolerates OpenSSL
`TRUSTED CERTIFICATE` labels and ignores well-formed `X509 CRL` sections in the
same bundle. If the file is empty, unreadable, or malformed, the affected DevNow
HTTP or secure websocket connection reports a user-facing error that points
back to these environment variables.

## Notices

DevNow stores "do not show again" flags for some UI prompts under the `[notice]` table.

## Plan mode defaults

`plan_mode_reasoning_effort` lets you set a Plan-mode-specific default reasoning
effort override. When unset, Plan mode uses the built-in Plan preset default
(currently `medium`). When explicitly set (including `none`), it overrides the
Plan preset. The string value `none` means "no reasoning" (an explicit Plan
override), not "inherit the global default". There is currently no separate
config value for "follow the global default in Plan mode".

## Realtime start instructions

`experimental_realtime_start_instructions` lets you replace the built-in
developer message DevNow inserts when realtime becomes active. It only affects
the realtime start message in prompt history and does not change websocket
backend prompt settings or the realtime end/inactive message.

Ctrl+C/Ctrl+D quitting uses a ~1 second double-press hint (`ctrl + c again to quit`).
