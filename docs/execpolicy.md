# Execution policy

Execution policies control which commands DevoNow may run automatically and which require approval. Start with `approval_policy = "on-request"` and `sandbox_mode = "workspace-write"` in `$CODEX_HOME/config.toml`.

Treat permissive policies as security-sensitive. Never disable approvals or sandboxing unless DevoNow is already running inside an isolated environment you control.
