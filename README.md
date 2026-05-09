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
- logic changes with no nearby test movement.

The goal is not to replace `git diff`. The goal is to answer:

```text
Which changed symbols deserve attention first, and why?
```

`sniffdiff` is intentionally not a repo knowledge graph, hosted service,
dashboard, AI reviewer, or broad multi-language analyzer.

## Status

Early MVP. Python only. Local Git only.

The crate is versioned as `0.0.1` while the fact model and output format settle.

## Install

From a local checkout:

```sh
cargo install --path .
```

Then run from any Git repo:

```sh
sniffdiff main..HEAD
```

Until the crate is published to crates.io, install directly from the repository:

```sh
cargo install --git https://github.com/wlamnorman/sniffdiff
```

## Usage

Compare a range in the current repository:

```sh
sniffdiff main..HEAD
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

Keep the same report shape but add more per-item facts:

```sh
sniffdiff main..HEAD --verbose
```

Emit exhaustive JSON facts for tools:

```sh
sniffdiff main..HEAD --json
```

## Example Output

```text
Sniffed the diff... 🐽
scope: 11 changed files, 16 changed symbols, 2 changed test files

- src/features.py::build_features
  change: public signature changed; logic changed
  signature: build_features(rows) -> build_features(rows, *, strict=False, source="unknown")
  complexity: increased; branches 0 -> 2; nesting 1 -> 2
  changed_tests: tests/test_features.py::test_build_features (1 callsite), tests/test_features.py::test_build_features_skips_missing_names (1 callsite)
  unchanged_callers: src/api.py::preview (1 callsite), src/batch.py::build_batch (1 callsite), src/pipeline.py::run_pipeline (1 callsite), src/predict.py::predict (1 callsite)
  changed_callers: src/reporting.py::summarize (1 callsite), src/train.py::train (1 callsite)
- src/validators.py::validate_row
  change: public signature changed; logic changed
  signature: validate_row(row) -> validate_row(row, *, strict=False)
  complexity: increased; branches 1 -> 4; nesting 1 -> 2
  tests: no nearby test movement
  changed_callers: src/validators.py::is_ready (1 callsite)
- src/features.py::Formatter.format_name
  change: public signature changed; logic changed
  signature: format_name(self, name) -> format_name(self, name, *, uppercase=False, fallback="unknown")
  complexity: increased; branches 1 -> 2
  changed_callers: src/features.py::Formatter.format_many (1 callsite)
- src/scoring.py::score_features
  change: logic changed
  complexity: increased; branches 0 -> 2; loops 0 -> 1; nesting 0 -> 2
  tests: no nearby test movement
  changed_callers: src/train.py::train (1 callsite)
- src/features.py::Formatter.format_many
  change: public signature changed; logic changed
  signature: format_many(self, names) -> format_many(self, names, *, uppercase=False)

omitted: 11 symbol changes; use --limit 10 for 5 more items, --json for 6 low-signal facts
```

## Demo

Generate and analyze a throwaway Python repo:

```sh
make demo
```

The demo includes signature changes, body changes, alias imports, module-alias
callers, changed tests, missing test movement, added/deleted symbols, and a
Git-detected file rename.

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
- No working-tree comparison yet.
- No hosted forge APIs.
- No persistent index.
- No full Python call graph.
- Import and call matching are static heuristics.
- Tests are parsed only to support production-symbol facts, not as primary
  review targets.

## Development

Run the normal checks:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Check package contents before publishing:

```sh
cargo package --list
cargo publish --dry-run
```

Build an optimized release binary:

```sh
cargo build --release
```

Run `sniffdiff --help` for the complete CLI surface.
