from __future__ import annotations

import re
import sys
from pathlib import Path


def extract_version(path: str) -> str:
    text = Path(path).read_text()
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    if not match:
        raise SystemExit(f"could not find version in {path}")
    return match.group(1)


def main() -> None:
    cargo_version = extract_version("Cargo.toml")
    python_version = extract_version("pyproject.toml")

    if cargo_version != python_version:
        print(
            "version mismatch: "
            f"Cargo.toml has {cargo_version}, pyproject.toml has {python_version}",
            file=sys.stderr,
        )
        raise SystemExit(1)

    print(f"version check ok: {cargo_version}")


if __name__ == "__main__":
    main()
