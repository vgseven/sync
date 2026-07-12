# sync

`sync` is a small Rust CLI that audits Python dependencies declared in a local
`pyproject.toml` and compares each required version against the latest release
published on PyPI.

The current implementation is intentionally narrow:

- It reads `[project].dependencies` from `pyproject.toml` in the current
  working directory.
- It extracts the package name and the declared version constraint from each
  dependency string.
- It requests the PyPI releases RSS feed for every package concurrently.
- It prefers the newest stable release and only falls back to a prerelease when
  no stable release exists in the feed.
- It prints the result as a terminal table with package name, required version,
  and latest available version.

## Current Scope

This repository currently contains a single executable defined in
[`src/main.rs`](./src/main.rs). There is no subcommand system or config file
support yet.

The CLI is designed around Python dependency strings such as:

- `requests>=2.32.3`
- `fastapi[standard]>=0.128.2`
- `python-jose[cryptography]>=3.5.0`
- `redis>=5.2.1 ; python_version >= "3.12"`

## How It Works

At runtime the CLI:

1. Reads `pyproject.toml`.
2. Parses the file with `toml_edit`.
3. Looks up `project.dependencies`.
4. Splits each dependency into:
   - package name
   - declared version constraint
5. Fetches `https://pypi.org/rss/project/<package>/releases.xml`.
6. Selects the first stable release from the feed.
7. Prints a formatted summary table.

The HTTP client uses conservative timeouts:

- connect timeout: 3 seconds
- request timeout: 8 seconds
- TCP keepalive: 30 seconds

## Running

From this directory:

```bash
cargo run
```

If you want the optimized binary:

```bash
cargo run --release
```

The CLI expects a `pyproject.toml` beside the executable's working directory.
If `[project].dependencies` is missing, execution fails with a clear error.

## Example Output

The output format is a table like this:

```text
  ---------------------------------------------------------
  Package                 Required               Latest
  ---------------------------------------------------------
  fastapi                 >=0.128.2              0.128.2
  redis                   >=5.2.1                6.4.0
  python-dotenv           >=1.0.1                1.1.1
  ---------------------------------------------------------
  3 packages  ·  1s
```

If a request fails, the package still appears in the table and the error is
shown inline.

## Project Files

- [`Cargo.toml`](./Cargo.toml): Rust package metadata and dependencies.
- [`Cargo.lock`](./Cargo.lock): locked crate versions.
- [`src/main.rs`](./src/main.rs): all runtime logic and unit tests.
- [`pyproject.toml`](./pyproject.toml): the Python dependency source inspected
  by the CLI.

## Limitations

The current codebase has several intentional or existing constraints:

- It only reads `project.dependencies`; it does not inspect optional
  dependencies, dependency groups, or tool-specific sections.
- It assumes dependency strings are simple enough to parse with delimiter-based
  splitting rather than full PEP 508 parsing.
- It only checks PyPI feeds, so private indexes and non-PyPI sources are not
  supported.
- It does not rewrite `pyproject.toml`; this is a read-only reporting tool.
- It reads a fixed filename (`pyproject.toml`) instead of accepting a path
  argument.
- It currently relies on network access to PyPI at runtime.

## Tests

Unit tests live in [`src/main.rs`](./src/main.rs) and currently cover the
release-selection behavior:

- prefer stable releases over prereleases
- fall back to prereleases when no stable release exists

Run them with:

```bash
cargo test
```

## Notes

- The checked `pyproject.toml` is whatever file lives in this service root.
- At the moment, the bundled `pyproject.toml` appears to be application
  metadata used as input data for the CLI rather than metadata for the Rust
  executable itself.
