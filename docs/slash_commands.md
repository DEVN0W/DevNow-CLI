# Slash commands

For an overview of Codex CLI slash commands, see [this documentation](https://developers.openai.com/codex/cli/slash-commands).

## DevNow mode commands

- `/mode` opens the operating-mode picker for Single AI, manual Multi-Agent, and Multi-Agent Strategic.
- `/collab` is a compatibility alias for `/mode`.
- `/plan` enters Plan mode without adding a fourth top-level operating mode.

Single AI is the startup default. Manual Multi-Agent requires a valid
`CODEX_HOME/agentic.toml`; it never starts a child agent without an explicit
agent tool call. Multi-Agent Strategic requires a valid
`CODEX_HOME/multi_agent.toml` and automatically runs its configured workflow.
