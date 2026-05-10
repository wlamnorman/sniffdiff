# Click NoSuchCommand Suggestions

This example runs against a real Click commit:

```text
831c8f0948af519e45b90801d7430ff25451f972
Add NoSuchCommand exception with suggestions for misspelled commands (#3228)
```

Run from the repository root:

```sh
make example-click
```

The example clones <https://github.com/pallets/click> into `target/`, then runs
`sniffdiff` over the commit and its parent. No Click source code is vendored in
this repository.
