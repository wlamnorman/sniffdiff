# Releasing

`sniffdiff` is distributed through both crates.io and PyPI.

Do not publish from an uncommitted worktree. Tagging and publishing should happen
from the exact commit that users will see on GitHub.

## Shared Checks

```sh
make pre-release
git diff --check
make example
make examples
```

`make preflight` is the local WIP-friendly variant. It allows a dirty worktree
for the crates.io dry run. Use `make pre-release` before tagging or publishing.

## crates.io

```sh
cargo package --list
cargo publish --dry-run
cargo publish
```

## PyPI

The Python package uses `maturin` in binary mode. It ships the Rust CLI as the
`sniffdiff` executable and includes a small Python wrapper so this also works:

```sh
python -m sniffdiff main..HEAD
```

Build local PyPI distributions:

```sh
make python-dist
```

This builds:

- a source distribution;
- a wheel for the current host platform;
- a Linux x86_64 manylinux2014 wheel through Docker.

The Linux wheel target requires Docker. Windows wheels still need to be built on
a Windows machine or VM because the wheel contains a native `sniffdiff.exe`.
Run `make python-dist-host` on Windows and copy the resulting wheel into
`target/pypi-dist/` before uploading.

Inspect package metadata:

```sh
uvx twine check target/pypi-dist/*
```

Publish to TestPyPI first:

```sh
make python-upload-testpypi
```

Then install from TestPyPI in a clean environment and smoke test:

```sh
uv tool install --index-url https://test.pypi.org/simple/ sniffdiff
sniffdiff --help
```

Publish to PyPI:

```sh
make python-upload-pypi
```

Prefer PyPI trusted publishing through GitHub Actions once release automation is
worth the setup. Until then, use a short-lived PyPI API token scoped to the
`sniffdiff` project.
