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

For the dynamic multi-agent roster, copy `docs/multi-agent.sample.toml` to `CODEX_HOME/multi_agent.toml`.
`CODEX_HOME/multi_agent.toml` is the active file Codex reads at runtime; keep the sample as your editable template.
To add or remove agents, edit that TOML file directly.
Each `[[agents]]` entry can set `model_provider` to a provider id you define in `config.toml`.
That lets one roster use different backends, such as OpenAI, Azure OpenAI, or another compatible URL, per agent.
If you want Azure for every agent, just set each `model_provider = "azure-openai"`.
You can also control per-agent routing with fields like `next_on_pass` and `next_on_fail`.
The built-in runtime roles `quality ai`, `security ai`, and `execution tester` are available without extra setup so the roster can use them immediately.
At runtime, you only need to give the task once; the agent instructions handle the review, security, validation, summary, forwarding, install requests, and fix loop automatically.
