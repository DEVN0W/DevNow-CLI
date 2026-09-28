# Open-Source Release Checklist

DevNow Agent is distributed under the Apache License 2.0. Complete this checklist before publishing a source release or release artifact.

## Repository contents

- Keep [LICENSE](LICENSE), [NOTICE](NOTICE), [SECURITY.md](SECURITY.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), and [docs/contributing.md](docs/contributing.md) in the release.
- Preserve upstream copyright, license, and third-party attribution notices.
- Confirm new dependencies and vendored assets have compatible licenses and required notices.
- Do not rename third-party products, API identifiers, compatibility environment variables, or upstream notices solely for branding.

## Sensitive data

- Confirm no API keys, tokens, credentials, private prompts, customer data, local configuration, or signing material is tracked.
- Keep credentials in environment variables. The files in `docs/*.sample.toml` must contain placeholders only.
- Review the complete release archive, including generated files and repository history, before publishing.

## Validation

- Build the release artifacts on every supported platform.
- Run the checks documented in [docs/contributing.md](docs/contributing.md) for changed crates.
- Verify that a clean checkout can follow the installation instructions in [README.md](README.md).
- Test Single AI, Multi-Agent, and Multi-Agent Strategic using the documented sample files.
- Verify links, screenshots, issue templates, and the private vulnerability-reporting URL.

## Release notes

- Summarize user-visible changes and known limitations.
- Identify breaking configuration, command, or protocol changes and provide migration instructions.
- Record the source revision used to build each artifact.
- Publish checksums for downloadable binaries and sign artifacts when release infrastructure supports it.

This checklist is release guidance, not a substitute for legal or security review.
