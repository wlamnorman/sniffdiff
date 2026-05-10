# Fact Model

`sniffdiff` keeps factual extraction separate from scoring. The current output is
intended to be review-relevant facts, not a ranking.

## Snapshots

Each side of the comparison is parsed independently into a `Snapshot`. A side
can be a Git ref, the Git index, or the working tree:

- files considered;
- files skipped by the default file filter;
- files parsed;
- files with parse errors;
- parse-error file paths;
- extracted symbols.

Python parse errors fail analysis by default. `--allow-parse-errors` returns
partial facts for tool/debug workflows.

## Symbol Identity

Symbols are compared with an exact key:

```text
file + qualified_name
```

Examples:

```text
src/features.py::build_features
src/features.py::Formatter.format_name
```

The implementation uses a deterministic map join:

```text
before only -> deleted
after only  -> added
both        -> compare signature/body/path facts
```

## Path Changes

`path_changed` is deliberately narrow:

- Git reported a file rename;
- the symbol qualified name stayed the same;
- the symbol key was normalized from the old path to the new path.

`path_changed` does not mean semantic move detection. Symbol renames and symbols
moved without a Git-detected file rename currently appear as `deleted` + `added`.

## Symbol Facts

Each changed symbol includes:

- `kind`: `function`, `method`, or `class`;
- `visibility`: `private`, `internal`, or `public`.

Visibility is heuristic. A private symbol has a non-dunder qualified-name
component starting with `_`. An internal symbol lives under a path component
starting with `_`. Everything else is currently treated as public.

Each extracted function or method symbol also carries structured signature
facts:

- whether the function is `async`;
- parameter names;
- parameter kind: positional-only, positional-or-keyword, keyword-only, varargs,
  or kwargs;
- whether each parameter has a default;
- the normalized default value, when present;
- parameter annotations;
- return annotation.

Symbols also carry raw complexity metrics:

- function length in lines;
- branch count;
- loop count;
- boolean operator count;
- exception handler count;
- match count;
- with count;
- max nesting depth.

These are raw facts only. They are not risk scores.

The JSON keeps the raw deltas. The concise text report only displays structural
complexity metrics whose absolute delta is at least 2, to avoid over-weighting
one-point noise.

The concise text report should render structural movement rather than a single
opaque complexity total, for example:

```text
complexity: increased; branches 2 -> 5; nesting 1 -> 3
```

This is intentionally simple. Future versions can incorporate exports, imports,
module conventions, and config.

## Signature and Complexity Deltas

When a symbol exists on both sides of the diff, `sniffdiff` compares structured
signature facts and complexity metrics.

Signature deltas include:

- parameters added;
- parameters removed;
- whether shared parameters were reordered;
- parameter kind changes;
- parameter default changes;
- parameter annotation changes;
- return annotation changes;
- async/sync changes.

Complexity deltas include the before metric set, the after metric set, and one
numeric delta per metric.

## Reference Facts

Production references are extracted from the before and after production
snapshots. The Python adapter tracks simple import bindings and aliases so
references can be matched with better precision than short-name matching alone.
Bindings are scope-aware enough to keep function-local imports from leaking into
later functions.

Current reference kinds:

- `import`: `import x`;
- `from_import`: `from x import y`;
- `call`: `y(...)`;
- `attribute`: `x.y(...)`.

Current Python import resolution handles common direct and module-style calls,
including:

- `from pkg.features import build_features` followed by `build_features(...)`;
- `from pkg.features import build_features as make_features` followed by
  `make_features(...)`;
- `import pkg.features as features` followed by `features.build_features(...)`;
- `import pkg.features` followed by `pkg.features.build_features(...)`;
- `from pkg import features` followed by `features.build_features(...)`.
- relative forms such as `from .. import features` followed by
  `features.build_features(...)`.

Import bindings are scoped. Function-local imports are visible inside nested
functions, but not to later sibling functions. Class-body imports are visible
inside the class body, but are not treated as lexical bindings inside methods.

Each reference includes:

- local name;
- raw module text when present;
- resolved module when an import binding is known;
- resolved symbol name when an import binding is known;
- resolution status: `resolved` or `unresolved`.

Each changed symbol includes:

- total reference count;
- resolved and unresolved reference counts;
- count by reference kind;
- unique reference files;
- changed reference files;
- unchanged reference files.

Unresolved calls can still match by short name as a fallback, but resolved
module/name matches are preferred.

## Test Facts

Tests remain excluded from primary symbol changes and production reference
facts. They are parsed in a separate support pass at `head` so `sniffdiff` can
report test references to changed production symbols without treating test
functions as review targets.

`test_facts` includes:

- changed test files;
- changed test file count;
- parsed test file count;
- test parse-error count and paths;
- production files with nearby test movement;
- production files without nearby test movement.

Each symbol change also includes `test_references_after`, using the same
reference-fact shape as production references. The concise report may render:

- `changed_tests`: changed test functions that call the changed production
  symbol;
- `unchanged_tests`: existing unchanged test functions that still call the
  changed production symbol;
- `tests: no direct test references found` when no changed or unchanged test
  reference matched the changed production symbol.

Nearby test movement is intentionally heuristic. For now, `tests/test_features.py`
is considered nearby `features.py`.

## Review Signals

Review signals are factual labels derived from raw facts. They are not a score.

Current signals:

- `public_signature_changed`;
- `public_symbol_added`;
- `public_symbol_deleted`;
- `type_annotations_changed`;
- `signature_changed_with_unchanged_callers`;
- `complexity_increased`;
- `implementation_changed_without_test_movement`;
- `path_changed_only`.

Symbol changes are ordered by these signal weights to make the output more
review-oriented without introducing a numeric score.

`complexity_increased` currently uses the same threshold as the concise report:
at least one structural complexity metric must increase by 2 or more.

## Output

Default output is intentionally compact: it summarizes the scope, high-signal
review items, caller context, test movement facts, and an omitted-count. The
default item limit is 5. `--limit N` or `--limit all` controls how many report
items are shown. `--caller-preview-limit N` controls how many caller and test
references are previewed inside each report item. `--verbose` keeps the same
report shape but adds extra per-item facts. Exhaustive raw facts belong in
`--json`.

Import references remain available as raw JSON facts, but dependency/import
changes are not shown in the text report for now. Plain import deltas were too
weak as review guidance without project-specific architecture boundaries.

The `unchanged_callers` and `changed_callers` fields list call sites for a
changed symbol:

- `unchanged_callers`: call sites in files that were not changed in the Git
  range;
- `changed_callers`: call sites in files that were changed in the Git range.

These are derived from static call and attribute-call references in the after
snapshot. Import statements are not listed as callers. Each matched reference
line is mapped to the innermost parsed symbol whose line range contains it, so
the output can show `path.py::function_name`. If no enclosing symbol is found,
the output falls back to `path.py:line`.

Caller labels include the number of matched call sites inside that caller
symbol.

The `changed_tests` and `unchanged_tests` fields use the same call-site labeling
rules, but come from the separate test-reference pass.

## File Filtering

Tests are skipped from symbol and reference extraction by default:

- `tests/**`;
- `test/**`;
- `test_*.py`;
- `*_test.py`.

Raw Git changed files are still preserved so test movement can be reported
without making test symbols primary review facts.
