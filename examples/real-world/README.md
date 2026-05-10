# Real-World Examples

These examples run `sniffdiff` against real commits from well-known Python
projects.

The examples intentionally do not vendor upstream source code. Each make target
clones the upstream repository into `target/real-world-examples/`, fetches a
specific public commit plus its parent, and runs `sniffdiff` locally.

That keeps this repository small and keeps upstream code under its original
license.

## Requests: Digest Security Flag

Upstream:

- repository: <https://github.com/psf/requests>
- commit: `a044b020dea43230585126901684a0f30ec635a8`
- license: Apache-2.0
- why this is useful: a small implementation change where `sniffdiff` should
  point at the changed digest-auth function and its local production callers.

Run:

```sh
make example-requests
```

## Click: NoSuchCommand Suggestions

Upstream:

- repository: <https://github.com/pallets/click>
- commit: `831c8f0948af519e45b90801d7430ff25451f972`
- license: BSD-3-Clause
- why this is useful: a medium-sized change where `sniffdiff` should surface
  exception/parser behavior, changed tests, and caller context.

Run:

```sh
make example-click
```

## Run Both

```sh
make examples
```
