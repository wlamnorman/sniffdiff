# sniffdiff Design Outline

`sniffdiff` is a Rust CLI for identifying review attention hotspots between two
Git refs. The MVP is intentionally lean: load Python files at two refs, parse
symbols with tree-sitter, and build the boundaries needed for later change
detection, complexity metrics, references, and scoring.

See `docs/positioning.md` for the product boundary. In short, `sniffdiff` should
stay a deterministic structural diff-facts CLI, not a repo knowledge graph,
MCP server, dashboard, or AI reviewer.

## First-Version Scope

- Local Git repositories only.
- Python only.
- No near-term multi-language roadmap.
- No hosting APIs.
- No AI.
- No final scoring model yet.
- Structured review report by default and verbose reviewer facts on request.

## Pipeline

```text
CLI refs
  -> GitBackend
  -> LanguageAdapter
  -> Symbol extraction
  -> Change detection
  -> Reference extraction
  -> Review facts
  -> Reviewer renderer
```

## Code Layout

```text
src/
  analysis/
    mod.rs          # orchestration and snapshot parsing
    facts.rs        # Analysis, Snapshot, SymbolChange, fact structs
    diff.rs         # exact-key symbol diff and path_changed logic
    filter.rs       # default file filtering
    references.rs   # per-symbol reference fact aggregation
  git.rs            # shell-based Git backend
  language/
    mod.rs          # LanguageAdapter trait
    types.rs        # shared symbol/reference value types
  python/
    mod.rs          # PythonAdapter and parser setup
    symbols.rs      # Python symbol extraction
    references.rs   # Python reference extraction
```

The current code parses both sides of the range and reports factual symbol
changes. Complexity, test coverage facts, and scoring are intentionally
deferred.

## Core Data Needed

See `docs/facts.md` for the current pre-scoring fact model.

The analyzer should preserve factual attribution before it tries to score
anything. The current model starts with:

- `Analysis`: top-level result for a base/head comparison.
- `Snapshot`: parsed state of one Git ref.
- `Symbol`: language-adapter output for a function, method, or class.
- `SymbolId`: stable comparison key, currently `file + qualified_name`.
- `SymbolChange`: before/after fact record for one changed symbol.
- `ChangeKind`: factual change labels such as `added`, `deleted`,
  `path_changed`, `body_changed`, and `signature_changed`.
- `SymbolReferenceFacts`: heuristic after-snapshot reference counts and file
  sets attached to each changed symbol.

Later impact scoring should consume these facts rather than recalculate them.
That lets review output explain where risk came from:

- local symbol body changed;
- public signature changed;
- complexity changed;
- references exist in unchanged files;
- nearby tests did or did not change.

## Git Backend

The current backend is `ShellGit`, which runs the installed `git` executable. `sniffdiff` invokes commands such as `git diff --name-status`, `git ls-tree`, and `git show` as subprocesses rather
than linking directly to a Git library.

Why start here:

- Git CLI behavior is familiar and battle-tested.
- It keeps the MVP focused on analysis rather than Git object plumbing.
- It handles normal repository config and credentials the same way developers
  already use Git locally.
- It can be hidden behind a trait from day one.

The trait is:

```rust
trait GitBackend {
    fn changed_files(&self, base: &str, head: &str) -> Result<Vec<ChangedFile>>;
    fn list_files_at_ref(&self, git_ref: &str, extensions: &[&str]) -> Result<Vec<PathBuf>>;
    fn read_file_at_ref(&self, git_ref: &str, path: &Path) -> Result<Option<String>>;
}
```

Later, this can be replaced or supplemented by a `git2` backend without changing
the parsing or analysis layers.

## What `git2` Is

`git2` is the main Rust binding to `libgit2`, a C library that implements Git
operations as an embeddable API. Instead of spawning `git`, the program opens
the repository and reads commits, trees, blobs, and diffs directly.

Potential advantages:

- No dependency on a `git` executable being installed.
- More structured API for commits, trees, blobs, and diffs.
- Potentially better performance once we optimize.
- Fewer subprocess calls.

Tradeoffs:

- More upfront complexity.
- Some behavior may differ subtly from the Git CLI users expect.
- Authentication and config behavior can be less familiar.
- The MVP does not need the extra control yet.

Hence we keep `GitBackend` as the boundary and start with `ShellGit`.

## Language Adapter Boundary

Language-specific behavior belongs behind `LanguageAdapter`:

```rust
trait LanguageAdapter {
    fn language_name(&self) -> &'static str;
    fn file_extensions(&self) -> &'static [&'static str];
    fn parse_file(&self, file: PathBuf, source: String) -> Result<ParsedFile>;
    fn extract_symbols(&self, parsed: &ParsedFile) -> Result<Vec<Symbol>>;
    fn extract_references(&self, parsed: &ParsedFile) -> Result<Vec<Reference>>;
}
```

This keeps the analyzer boundary clean, but `sniffdiff` is intentionally
Python-first:

- Python uses tree-sitter-python now.
- Other language adapters are not a near-term goal.
- The adapter boundary exists to keep parsing details isolated, not to chase
  broad language coverage before Python facts are excellent.

## Tree-Sitter and tree-sitter-python

Tree-sitter is an incremental parser generator and parsing library. It provides
concrete syntax trees for many languages. `tree-sitter-python` is the Python
grammar package used by the Rust `tree-sitter` runtime.

Why it is a strong MVP choice:

- Fast enough for whole-repository parsing.
- Widely used in editors and code tools.
- Good language coverage if adapters expand later.
- Robust on incomplete or imperfect code.
- Gives exact byte ranges and line ranges for syntax nodes.

What it does not provide:

- Full semantic resolution.
- Type information.
- Perfect import resolution.
- Knowledge that a call named `foo()` definitely points to a specific symbol.

That limitation is acceptable for `sniffdiff` because the product is heuristic
review attention, not a compiler-grade impact analyzer.

Can it be changed later? Yes, if the adapter boundary is preserved. The rest of
the system should consume `Symbol` and `Reference` facts, not tree-sitter nodes
directly. A future Python adapter could use Ruff, Pyright, Jedi, or a hybrid
approach without replacing Git or scoring.

## Ruff and basedpyright Options

Ruff is especially interesting because it is written in Rust and has its own
Python parser, AST, resolver, and lint infrastructure. If we want deeper Python
knowledge while staying native-Rust, the likely path is a future
`RuffPythonAdapter` or a hybrid adapter that uses Ruff crates for Python syntax
and semantic facts while still returning `sniffdiff`'s own `Symbol` and
`Reference` structs.

Potential Ruff uses:

- more Python-native AST than tree-sitter;
- import and binding information;
- project-aware configuration discovery;
- eventually, reuse of lint-style traversal patterns.

Tradeoffs:

- Ruff internals may be less stable as a public library boundary than
  tree-sitter grammar crates;
- it is Python-specific, so it does not help the multi-language adapter story
  as directly as tree-sitter;
- adopting it early may couple the MVP to Ruff's internal model before we know
  exactly which facts `sniffdiff` needs.

basedpyright is better viewed as an optional semantic enrichment backend. It can
understand more about imports, types, definitions, references, and project
configuration than a syntax parser. The natural integration would be through
the language-server protocol or subprocess calls, not a native Rust crate.

Potential basedpyright uses:

- stronger import and reference resolution;
- type-aware call and attribute understanding;
- workspace-level Python project knowledge.

Tradeoffs:

- heavier runtime dependency;
- slower and more operationally complex than local parsing;
- less aligned with a lean first version;
- harder to make deterministic across user environments.

Recommendation: start with tree-sitter for the skeleton and core diff pipeline.
Keep the `LanguageAdapter` boundary strict. Revisit Ruff before implementing
serious Python reference detection. Treat basedpyright as a later optional
`SemanticBackend`, not the default parser.

## Parsing Model

The Python adapter should extract:

- classes
- functions
- methods
- signatures
- line ranges
- body hashes
- later: structural complexity metrics
- later: imports and references

Initial symbol identity:

```text
path + qualified_name
```

Examples:

```text
src/features.py::build_features
src/models.py::User.normalize_email
```

This is simple and rename-hostile, which is acceptable for the first version.

Current Python parser contracts:

- top-level `def` and `async def` become function symbols;
- class-level `def` becomes method symbols qualified by class name;
- decorators do not become part of the symbol name or signature;
- multiline signatures are captured through the closing colon;
- nested functions are intentionally ignored for now;
- class body hashes exclude nested method/class definitions, so method-only
  edits do not create extra class-level symbol changes.
- files with syntax errors are marked, but recoverable symbols are still
  extracted.

Current parse-error policy:

- analysis fails by default if either snapshot has Python parse errors;
- the error lists parse-error file paths and tells the user to fix syntax first;
- `--allow-parse-errors` returns partial facts for tool/debug workflows;
- partial facts are explicitly opt-in because parse errors can make symbol and
  reference facts incomplete.

Current Python reference contracts:

- `import x` emits an import reference for `x`;
- `from x import y` emits a from-import reference for `y` with module `x`;
- `y(...)` emits a direct call reference for `y`;
- `x.y(...)` emits an attribute call reference for `y` with module/object `x`;
- per-symbol reference facts are name-based heuristics, not semantic resolution.

Current path-change contract:

- `path_changed` means Git reported a file rename and the symbol qualified name
  stayed the same under the new file path;
- `path_changed` does not mean `sniffdiff` detected a semantic symbol move;
- symbol renames are not matched yet and appear as `deleted` + `added`;
- symbols moved across files without a Git-detected file rename are not matched
  yet and appear as `deleted` + `added`;
- heuristic symbol matching by name/body similarity is intentionally deferred.

Current file filtering contracts:

- test files are skipped from symbol and reference extraction by default;
- `tests/**`, `test/**`, `test_*.py`, and `*_test.py` count as test paths;
- raw Git changed files are still preserved, so test facts can be added later
  without putting test symbols in the primary review facts;
- the filter is intentionally separate from the Python adapter so later config
  can add user-defined ignore patterns.

## Near-Term Build Order

1. Keep the CLI and backend/adaptor traits compiling.
2. Make Python symbol extraction reliable.
3. Add before/after symbol indexing.
4. Detect added, deleted, body-changed, and signature-changed symbols.
5. Add simple structural complexity facts.
6. Add heuristic reference extraction.
7. Only then design scoring and review output.

## Manual Demo Repo

For local testing without finding another project, generate a throwaway Python
repo with two commits:

```sh
scripts/create-demo-repo.sh
```

Or run the full local demo:

```sh
make demo
```

The script prints exact `base` and `head` SHAs plus commands like:

```sh
cargo run -- --repo target/demo-python-repo BASE..HEAD
cargo run -- --repo target/demo-python-repo BASE..HEAD --verbose
```

`make demo` creates the demo repo and prints the default review report.

The demo intentionally includes:

- multiple function body and signature changes;
- direct imports, aliased imports, and module-aliased callers;
- import impact with new external, new internal, and removed imports;
- unchanged callers of changed signatures;
- a deleted helper;
- an added helper;
- a method signature change;
- body-only complexity increases;
- a Git-detected file rename with unchanged symbol logic;
- changed test files and production files without nearby test movement.
