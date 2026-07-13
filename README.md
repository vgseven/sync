# relay-sync

`relay-sync` is a Rust CLI for checking and safely updating dependency versions
in Python and Node.js project manifests.

The binary is named `relay-sync` instead of `sync` because Unix-like systems
already ship a system command named `sync`.

## Current Working Scope

This codebase currently supports:

- `check`: report dependency status without changing files.
- `update`: rewrite supported manifest declarations to the registry latest
  version.
- Python registry checks through the PyPI JSON API.
- Node.js registry checks through the npm registry `latest` dist-tag.
- Table and JSON output.
- macOS, Linux, and Windows-compatible Rust code paths.

Supported manifests:

- `package.json`
- `pyproject.toml`
- `Pipfile`
- `requirements.txt`
- `requirements.in`
- `setup.cfg`

Supported dependency sections:

- Node.js: `dependencies`, `devDependencies`, `optionalDependencies`,
  `peerDependencies`.
- PEP 621: `[project].dependencies`,
  `[project.optional-dependencies]`.
- PEP 735-style dependency groups: `[dependency-groups]`.
- Poetry: `[tool.poetry.dependencies]`,
  `[tool.poetry.dev-dependencies]`,
  `[tool.poetry.group.<name>.dependencies]`.
- Pipfile: `[packages]`, `[dev-packages]`.
- requirements files: simple PEP 508 requirement lines.
- setup.cfg: `install_requires`, `setup_requires`, `tests_require`, and
  `options.extras_require`.

## Safety Rules

`relay-sync update` only rewrites declarations it can preserve safely.

It updates simple constraints such as:

- `^1.2.3`
- `~1.2.3`
- `>=1.2.3`
- `==1.2.3`
- exact versions like `1.2.3`
- PEP 508 strings with extras or environment markers.

It skips:

- direct URL dependencies
- git dependencies
- local path dependencies
- workspace/file/link dependencies
- compound ranges such as `>=1,<2`
- upper-bound constraints such as `<2`
- wildcard constraints such as `1.*`
- unconstrained declarations such as `*` or `latest`

This keeps update mode conservative: if the CLI cannot prove a rewrite is
straightforward, it reports the dependency instead of editing it.

## Usage

From this directory:

```bash
cargo run -- check
```

Check a specific project:

```bash
cargo run -- --path /path/to/project check
```

Check both Python and Node.js manifests recursively:

```bash
cargo run -- --path /path/to/project --recursive check
```

Return JSON:

```bash
cargo run -- --path /path/to/project --format json check
```

Update supported dependency declarations:

```bash
cargo run -- --path /path/to/project update
```

Preview updates without writing files:

```bash
cargo run -- --path /path/to/project update --dry-run
```

Only check or update selected packages:

```bash
cargo run -- --path /path/to/project check requests react
cargo run -- --path /path/to/project update requests react
```

## Exit Codes

- `0`: completed successfully.
- `1`: `check --fail-on-outdated` found outdated dependencies.
- `2`: CLI, parsing, registry, or write failure.

## Install On macOS

After a GitHub Release has been published, install the latest macOS binary:

```bash
curl -fsSL https://raw.githubusercontent.com/vgseven/sync/master/install.sh | bash
```

The installer supports Apple Silicon and Intel Macs. It downloads the matching
versioned release archive, verifies its SHA-256 checksum, and installs the
binary to `~/.local/bin/relay-sync` by default. It does not require `sudo`.

Install a specific release or choose a different destination:

```bash
curl -fsSL https://raw.githubusercontent.com/vgseven/sync/master/install.sh | \
  RELAY_SYNC_VERSION=v0.2.0 RELAY_SYNC_INSTALL_DIR="$HOME/bin" bash
```

If the install directory is not already in your shell path, add this to
`~/.zshrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

## Release Automation

Pushing a tag in the form `v<version>` triggers
[`release.yml`](./.github/workflows/release.yml). The tag must match the
version in `Cargo.toml`. The workflow builds native Apple Silicon and Intel
macOS archives, generates individual checksums plus `SHA256SUMS`, and creates a
GitHub Release.

The default release workflow does not sign or notarize the binary. Add Apple
Developer ID signing and notarization before presenting the installer as a
fully trusted public macOS distribution channel.

## Release Notes For Current State

This repository is now structured as a real CLI package:

- package name: `relay-sync`
- binary name: `relay-sync`
- current version: `0.2.0`
- primary implementation entrypoint: `src/app.rs`
- manifest parsing: `src/manifest.rs`
- version parsing and safe replacement logic: `src/version.rs`
- registry calls: `src/registry.rs`

The current code has release packaging and a macOS installer. The first public
release still requires a pushed version tag, successful GitHub Actions run, and
Apple signing/notarization if Gatekeeper-trusted distribution is required.

## Verification

Run:

```bash
cargo fmt --check
cargo check
cargo test
```

## Planned Production Work

The interrupted production plan is tracked in [`ROADMAP.md`](./ROADMAP.md).
Those items are intentionally not implemented in this pass; this pass only
stabilizes the code that was already started.
