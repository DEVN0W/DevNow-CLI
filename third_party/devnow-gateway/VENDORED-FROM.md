# Vendored Source

This component is vendored from `llm-rosetta` and retains its upstream MIT
license in `LICENSE`.

- Upstream project: `Oaklight/llm-rosetta`
- Vendored directly into: `third_party/devnow-gateway`
- DevNow runtime name: `devnow-gateway`
- Internal Python import namespace: `llm_rosetta` (intentionally retained to
  minimize converter churn)
- Vendored on: 2026-08-27

DevNow-specific changes are limited to packaging, runtime branding,
localhost-safe defaults, and managed lifecycle integration. The conversion
pipeline remains intact to preserve behavior parity with the source copy.

## Bundled compatibility dependency

`src/typing_extensions.py` is vendored from `typing_extensions` 4.15.0 so the
source-launched gateway works with Python 3.10 without a separate pip install.
Its PSF license is preserved at `third_party/typing_extensions/LICENSE`.
