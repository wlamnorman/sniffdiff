# Requests Digest Security Flag

This example runs against a real Requests commit:

```text
a044b020dea43230585126901684a0f30ec635a8
Move DigestAuth hash algorithms to use usedforsecurity=False (#7310)
```

Run from the repository root:

```sh
make example-requests
```

The example clones <https://github.com/psf/requests> into `target/`, then runs
`sniffdiff` over the commit and its parent. No Requests source code is vendored
in this repository.
