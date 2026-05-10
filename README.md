# sniffdiff

`sniffdiff` is a Rust CLI for reviewing Python diffs by symbol instead of by
line count.

It compares two local Git refs, parses the Python code before and after the
range, and prints a compact report of review facts:

- changed functions, methods, and classes;
- body versus signature changes;
- structural complexity movement;
- changed and unchanged callers;
- changed and unchanged tests that reference changed production symbols;
- implementation changes with no direct test references.

The goal is not to replace `git diff`. The goal is to answer:

```text
Which changed symbols deserve attention first, and why?
```

`sniffdiff` is intentionally not a repo knowledge graph, hosted service,
dashboard, AI reviewer, or broad multi-language analyzer.

## Quick Start

Install from crates.io:

```sh
cargo install sniffdiff
```

Or from PyPI:

```sh
uv tool install sniffdiff
pipx install sniffdiff
```

Requirement: `git` must be installed and available on `PATH`. `sniffdiff`
invokes the local Git CLI to read diffs and file contents; it does not vendor,
install, or redistribute Git.

Run it from a Python repo. The CLI is intentionally modeled after `git diff`:

```sh
sniffdiff              # like git diff: index -> working tree
sniffdiff --staged     # like git diff --staged: HEAD -> index
sniffdiff --cached     # alias for --staged
sniffdiff HEAD         # like git diff HEAD: HEAD -> working tree
sniffdiff main         # like git diff main: main -> working tree
sniffdiff main..HEAD   # like git diff main..HEAD: main -> HEAD
sniffdiff main...HEAD  # like git diff main...HEAD: merge-base(main, HEAD) -> HEAD
```

Common options:

```sh
sniffdiff main --limit 10
sniffdiff main --caller-preview-limit 8
sniffdiff main --verbosity verbose
sniffdiff main --json
```

Analyze another repository:

```sh
sniffdiff --repo ../some-python-repo main..HEAD
```

When installed as a Python package, `python -m sniffdiff main..HEAD` delegates
to the same Rust binary. For local wrapper testing, set `SNIFFDIFF_BIN` to an
explicit binary path.

## Example Output

```yaml
schema_version: 1
verbosity: normal
scope:
  changed_files: 11
  changed_symbols: 16
  changed_test_files: 2
inspect:
- symbol: src/features.py::build_features
  changes:
  - public signature
  - implementation
  signature:
    before: build_features(rows)
    after: build_features(rows, *, strict=False, source="unknown")
  complexity:
    status: increased
    metrics:
    - name: branches
      before: 0
      after: 2
  changed_tests:
  - tests/test_features.py::test_build_features
  - tests/test_features.py::test_build_features_skips_missing_names
  unchanged_callers:
  - src/api.py::preview
  - src/batch.py::build_batch
  - src/pipeline.py::run_pipeline
  - src/predict.py::predict
  changed_callers:
  - src/reporting.py::summarize
  - src/train.py::train
- symbol: src/validators.py::validate_row
  changes:
  - public signature
  - implementation
  signature:
    before: validate_row(row)
    after: validate_row(row, *, strict=False)
  complexity:
    status: increased
    metrics:
    - name: branches
      before: 1
      after: 4
  tests: no direct test references found
  changed_callers:
  - src/validators.py::is_ready
- symbol: src/scoring.py::score_features
  changes:
  - implementation
  complexity:
    status: increased
    metrics:
    - name: branches
      before: 0
      after: 2
    - name: nesting
      before: 0
      after: 2
  tests: no direct test references found
  changed_callers:
  - src/train.py::train
- symbol: src/features.py::Formatter.format_many
  changes:
  - public signature
  signature:
    before: format_many(self, names)
    after: format_many(self, names, *, uppercase=False)
- symbol: src/features.py::Formatter.format_name
  changes:
  - public signature
  signature:
    before: format_name(self, name)
    after: format_name(self, name, *, uppercase=False, fallback="unknown")
  changed_callers:
  - src/features.py::Formatter.format_many
omitted:
  symbol_changes: 11
  high_signal: 9
  low_signal: 2
  hint: use --limit 14 to show all high-signal items, --verbosity full for 2 low-signal facts
```

## Status

Early MVP. Python only. Local Git only.

The crate is versioned as `0.0.1` while the fact model and output format settle.

## Install

From a local checkout:

```sh
cargo install --path .
```

Or install directly from the repository:

```sh
cargo install --git https://github.com/wlamnorman/sniffdiff
```

## Usage

Review unstaged changes, matching `git diff`:

```sh
sniffdiff
```

Review staged changes, matching `git diff --staged`:

```sh
sniffdiff --staged
sniffdiff --cached
```

Compare a base ref against the working tree, matching `git diff <ref>`:

```sh
sniffdiff main
```

Compare two refs, matching `git diff <ref1>..<ref2>`:

```sh
sniffdiff main..HEAD
```

Compare a branch from its merge base, matching `git diff <ref1>...<ref2>`:

```sh
sniffdiff main...HEAD
```

Analyze another repository:

```sh
sniffdiff --repo ../some-python-repo main..HEAD
```

Use explicit refs instead of `base..head`:

```sh
sniffdiff --repo ../some-python-repo --base main --head HEAD
```

Show more report items:

```sh
sniffdiff main..HEAD --limit 10
sniffdiff main..HEAD --limit all
```

Show more caller/test references inside each report item:

```sh
sniffdiff main..HEAD --caller-preview-limit 8
```

Choose report verbosity:

```sh
sniffdiff main..HEAD --verbose
sniffdiff main..HEAD --verbosity verbose
sniffdiff main..HEAD --verbosity full
```

The default output format is YAML. Emit the same report model as JSON:

```sh
sniffdiff main..HEAD --json
sniffdiff main..HEAD --format json --verbosity full
```

When installed as a Python package, `python -m sniffdiff main..HEAD` delegates
to the same Rust binary. For local wrapper testing, set `SNIFFDIFF_BIN` to an
explicit binary path.

## Support Expectations

`sniffdiff` does not run Python code or depend on the Python interpreter in the
target repository. It parses Python source statically with `tree-sitter-python`,
so syntax support follows the bundled parser grammar rather than a local
`python` executable version. The current lockfile resolves `tree-sitter-python`
to `0.23.6`, whose grammar includes Python 3-era constructs such as structural
pattern matching (`match`/`case`), exception groups (`except*`), positional-only
and keyword-only parameter separators, f-string interpolation, and Python
3.12-style `type` alias statements and generic type parameters. It also retains
some legacy Python grammar support, but `sniffdiff` is tested and positioned as
a Python 3 source analyzer. Newer Python syntax should be treated as supported
only after it is accepted by the bundled parser and covered by fixtures.

Runtime is currently proportional to the amount of Python source in the compared
refs, not only to the number of changed files. The analyzer parses non-test
Python files at both `base` and `head`, then parses test files at `head` to
attach test-reference facts. It is intended to be fast enough for local review
on typical Python packages, but large-repo performance is not yet benchmarked or
optimized.

Monorepos are supported when the repository is local and the refs are available,
but the tool is monorepo-compatible today rather than monorepo-optimized. Future
monorepo work should add repeatable `--path` scopes, `--exclude` and config
support, smarter changed-file scoping, parallel parsing, batched Git object
reads, optional blob-based caching, and timing/file-count fields in JSON output.

## Examples

Generate and analyze a throwaway Python repo:

```sh
make example
```

`make demo` is kept as an alias. The built-in example is deterministic and
offline. It includes signature changes, body changes, alias imports,
module-alias callers, changed tests, missing test movement, added/deleted
symbols, and a Git-detected file rename.

Run against real commits from well-known Python packages:

```sh
make example-requests
make example-click
make examples          # all real-world examples
```

Real-world examples live under `examples/real-world/`. They clone upstream
repositories into `target/` at run time instead of vendoring package source into
this repo, so they require network access the first time they run.

## GitHub Action

This repository includes `.github/workflows/sniff.yml`, which runs `sniffdiff`
against pull requests and writes the report to the GitHub Actions step summary.
It is intentionally log-first for now: no bot comments, no tokens beyond
read-only repository access, and no hosted service dependency.

## Parse Errors

By default, `sniffdiff` fails if either side of the range contains Python syntax
errors. That keeps the review facts honest.

For partial output while debugging a broken branch:

```sh
sniffdiff main..HEAD --allow-parse-errors
```

The report includes a `parse_errors:` line when partial facts were produced.

## Current Limits

- Python only.
- Local Git refs only.
- Git diff style comparisons are supported:
  - `sniffdiff` compares the Git index to the working tree;
  - `sniffdiff --staged` and `sniffdiff --cached` compare `HEAD` to the Git index;
  - `sniffdiff <ref>` compares that ref to the working tree;
  - `sniffdiff <ref1>..<ref2>` compares two refs;
  - `sniffdiff <ref1>...<ref2>` compares `merge-base(<ref1>, <ref2>)` to `<ref2>`.
- No hosted forge APIs.
- No persistent index.
- No full Python call graph.
- Import and call matching are static heuristics.
- Tests are parsed only to support production-symbol facts, not as primary
  review targets.

## Development

Run the normal checks:

```sh
make check
```

Run the fuller local preflight check, including a crates.io dry run that allows
a dirty worktree:

```sh
make preflight
```

Run the strict pre-release check from a clean worktree. This also checks that
the Rust and Python package versions match and builds the Python wheel/sdist:

```sh
make pre-release
```

See [docs/releasing.md](docs/releasing.md) for the crates.io and PyPI release
checklists.

Build an optimized release binary:

```sh
cargo build --release
```

Check the Python package wrapper:

```sh
PYTHONPATH=python SNIFFDIFF_BIN=target/debug/sniffdiff python -m sniffdiff --help
```

Build a Python wheel locally when `maturin` is available:

```sh
maturin build
```

Run `sniffdiff --help` for the complete CLI surface.
