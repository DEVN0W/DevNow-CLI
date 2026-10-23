# Authentication

DevoNow uses environment variables for provider credentials. Never write API keys into a committed configuration file.

Common variables include:

- `OPENAI_API_KEY` for OpenAI
- `DEVNOW_CLAUDE_API_KEY` for Claude through the DevoNow gateway
- `AZURE_OPENAI_API_KEY` for Azure OpenAI
- `AZURE_FOUNDRY_API_KEY` for Azure AI Foundry
- A custom variable named by `env_key` for any compatible provider

Ollama and LM Studio can run locally without a hosted-provider API key. See [configuration](config.md) for provider examples.
