# Positioning

`sniffdiff` is a deterministic structural diff-facts CLI.

Given a local Git range, it answers:

```text
What changed structurally, and what raw review facts should a reviewer or tool
inspect before forming a judgment?
```

The intended core command is:

```text
sniffdiff base..head
```

## What `sniffdiff` Is

- A local CLI for one Git range.
- A source of raw, deterministic review facts.
- A symbol-oriented companion to line-oriented `git diff`.
- A structured review report for reviewers, hooks, scripts, and coding tools.
- Python-first. Other languages are not a near-term goal.

The core facts should stay explainable:

- changed files;
- changed functions, methods, and classes;
- body versus signature changes;
- parameter-level signature deltas;
- raw complexity deltas;
- references to changed symbols;
- changed versus unchanged referencing files;
- test movement facts as a separate channel.

## What `sniffdiff` Is Not

`sniffdiff` should not become a general code intelligence platform.

It is not:

- a repo knowledge graph;
- an MCP server;
- a graph RAG system;
- an AI reviewer;
- a dashboard;
- a code search engine;
- a refactoring engine;
- a persistent index database.

Those are useful products, but they are not the narrow wedge for this tool.

## GitNexus Boundary

GitNexus appears to cover broad repo-intelligence workflows:

- impact analysis from a target symbol;
- process-grouped search;
- 360-degree symbol context;
- pre-commit change detection;
- multi-file rename support;
- MCP and tool workflows.

That overlap matters. `sniffdiff` should not compete by trying to become a
smaller GitNexus.

The narrower opportunity is diff-native review facts:

```text
GitNexus:  What depends on this symbol or process in the repo graph?
sniffdiff: What changed in this Git range, and what exact review facts changed?
```

For example, `sniffdiff` should aim to surface facts such as:

```text
symbol: src/features.py::build_features
changes: body_changed, signature_changed
signature: parameter added: strict
complexity: branch_count +2, max_nesting_depth +1
unchanged callers: src/train.py, src/predict.py
```

This is not a graph query result or final review judgment. It is a compact,
auditable fact record for one diff.

## Product Constraint

Before adding large features, ask:

```text
Does this improve deterministic facts for base..head?
```

If the answer is no, the feature probably belongs outside `sniffdiff` or in a
future integration.

## Near-Term Priorities

1. Keep stabilizing the output fact model instead of exposing internal structs.
2. Improve import-aware reference matching and unresolved-reference reporting.
3. Expand test facts as a separate channel, not as primary risk facts.
4. Keep complexity and signature changes as raw facts before numeric scoring.
5. Compare periodically against GitNexus to make sure the narrow wedge remains
   real.
