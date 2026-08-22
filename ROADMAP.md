# relay-sync Roadmap

These are planned items for turning the current working CLI into a production
release. They are not part of the current implementation.

## Release Packaging

- GitHub Actions CI and tag-triggered macOS release builds are implemented.
- Apple Silicon macOS `aarch64-apple-darwin` archives, checksums, GitHub
  Releases, and the cargo-dist-generated curl installer are implemented.
- Add Apple Developer ID signing, notarization, and CI secrets before calling
  the macOS installer Gatekeeper-trusted.
- Add and test `x86_64-apple-darwin` before documenting Intel macOS support.
- Resolve cargo-dist shell-installer verification on stock macOS: version 0.32
  checks SHA-256 only through `sha256sum`, while macOS ships `shasum -a 256`.
  Do not claim enforced checksum verification for the curl installer until this
  is supported upstream or a separately reviewed installer is adopted.
- Add Linux release targets; cargo-dist will extend the generated shell installer.
- Add `x86_64-pc-windows-msvc` releases and a PowerShell installer.
- Evaluate package-manager distribution through Homebrew, Scoop, Winget,
  Cargo, npm, or PyPI wrapper packages.

## Lockfile Updates

- Regenerate existing Node.js lockfiles with the detected package manager:
  - `package-lock.json` / `npm-shrinkwrap.json` through npm.
  - `pnpm-lock.yaml` through pnpm.
  - `yarn.lock` through yarn.
  - `bun.lock` / `bun.lockb` through bun.
- Regenerate existing Python lockfiles with the detected package manager:
  - `uv.lock` through uv.
  - `poetry.lock` through Poetry.
  - `pdm.lock` through PDM.
  - `Pipfile.lock` through Pipenv.
  - compiled requirements files through pip-tools.
- Add rollback behavior if manifest updates succeed but lockfile regeneration
  fails.

## Manifest Coverage

- Add workspace-aware Node.js discovery from `package.json` `workspaces`.
- Add package manager detection from `packageManager`.
- Add optional support for private registries and auth tokens.
- Add better handling for setup.cfg inline comma-separated dependency lists.
- Evaluate whether dynamic files such as `setup.py` should be reported as
  unsupported instead of silently ignored.
- Add explicit support notes for Conda, Poetry source tables, and custom PyPI
  indexes before claiming broader Python environment support.

## Safety And UX

- Add integration tests with local mock PyPI and npm registries.
- Add transaction snapshots for every file touched by update mode.
- Add `--allow-major`, `--allow-prerelease`, and policy controls.
- Add machine-readable update plans for CI use.
- Add a `doctor` command for checking required package managers before update.
- Add shell completions and manpage generation.

## Documentation

- Add installation docs after release artifacts exist.
- Add examples for Python-only, Node-only, and monorepo projects.
- Add a compatibility matrix for manifest sections and rewrite behavior.
- Add security notes for registry access, auth tokens, and update trust model.
