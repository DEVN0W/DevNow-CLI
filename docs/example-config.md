# Sample configuration

For the main `config.toml` sample, see [this documentation](https://developers.openai.com/codex/config-sample).

If you do not set a custom provider, Codex uses the built-in `openai` provider.
That is the “vanilla” path.
If you want Azure OpenAI or another compatible backend, add a provider block to `~/.codex/config.toml`.

```toml
[model_providers.azure-openai]
name = "Azure OpenAI"
base_url = "https://<your-resource>.openai.azure.com/openai"
env_key = "AZURE_OPENAI_API_KEY"
wire_api = "responses"
query_params = { api-version = "2025-04-01-preview" }
```

```toml
[model_providers.azure-foundry]
name = "Azure Foundry"
base_url = "https://<your-foundry-project-endpoint>/api/projects/<project-name>/openai/v1"
env_key = "AZURE_FOUNDRY_API_KEY"
wire_api = "responses"
extra_body = { agent_reference = { name = "emea-swd-multi-agent-ai", version = "2", type = "agent_reference" } }
```

Then point your profile at it:

```toml
[profiles.default]
model_provider = "azure-openai"
```

To switch later, just add another entry under `[model_providers.*]` and update
`model_provider` in the profile you want to use.
For Foundry, use the project endpoint plus the provider `extra_body` shown
above so Codex can add the `agent_reference` payload on that provider only.

For Manual Multi-Agent mode, copy `docs/agentic.sample.toml` to
`CODEX_HOME/agentic.toml`. Its `developer` entry is the default agent and uses
`gpt-5.4`. GPT entries reference the existing `openai` provider and Claude
entries reference the existing `claude` provider. Keep URLs and API keys in
`config.toml` or environment variables, never in `agentic.toml`. Manual agents
remain inactive until the root AI explicitly calls an agent tool.

For Multi-Agent Strategic mode, copy `docs/multi-agent.sample.toml` to
`CODEX_HOME/multi_agent.toml`. This file includes automatic routing fields such
as `next_on_pass` and `next_on_fail`; those fields are intentionally rejected in
the manual roster. Each `[[agents]]` entry can reference a provider id from
`config.toml`, including a custom Azure-compatible provider.
