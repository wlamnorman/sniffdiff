from __future__ import annotations

import re
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PERF_DIR = ROOT / "target" / "perf"
EXAMPLES_DIR = ROOT / "target" / "real-world-examples"
SNIFFDIFF = ROOT / "target" / "release" / "sniffdiff"


@dataclass(frozen=True)
class PerfCase:
    name: str
    repo_url: str
    commit: str
    limit: str


CASES = [
    PerfCase(
        name="requests-digest-security",
        repo_url="https://github.com/psf/requests.git",
        commit="a044b020dea43230585126901684a0f30ec635a8",
        limit="5",
    ),
    PerfCase(
        name="click-no-such-command",
        repo_url="https://github.com/pallets/click.git",
        commit="831c8f0948af519e45b90801d7430ff25451f972",
        limit="8",
    ),
]


def main() -> None:
    PERF_DIR.mkdir(parents=True, exist_ok=True)
    run(["cargo", "build", "--release"])

    print("perf smoke test")
    print(f"binary: {SNIFFDIFF.relative_to(ROOT)}")
    print(f"output: {PERF_DIR.relative_to(ROOT)}")
    print()

    for case in CASES:
        run_case(case)


def run_case(case: PerfCase) -> None:
    repo = ensure_repo(case)
    base = git(repo, "rev-parse", f"{case.commit}^")
    head = git(repo, "rev-parse", case.commit)
    stdout_path = PERF_DIR / f"{case.name}.out"
    stderr_path = PERF_DIR / f"{case.name}.timing"

    command = [
        str(SNIFFDIFF),
        "--repo",
        str(repo),
        f"{base}..{head}",
        "--limit",
        case.limit,
        "--caller-preview-limit",
        "4",
        "--timing",
    ]

    started_at = time.perf_counter()
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    elapsed = time.perf_counter() - started_at
    stdout_path.write_text(completed.stdout)
    stderr_path.write_text(completed.stderr)

    if completed.returncode != 0:
        sys.stderr.write(completed.stderr)
        raise SystemExit(completed.returncode)

    timings = parse_timings(completed.stderr)
    print(case.name)
    print(f"  repo: {case.repo_url}")
    print(f"  commit: {case.commit[:7]}")
    print(f"  wall: {elapsed:.2f}s")
    for key in [
        "total",
        "git_changed_files",
        "before_snapshot",
        "after_snapshot",
        "test_snapshot",
        "reference_facts",
    ]:
        if key in timings:
            print(f"  {key}: {timings[key]}")
    print(f"  report: {stdout_path.relative_to(ROOT)}")
    print(f"  timing: {stderr_path.relative_to(ROOT)}")
    print()


def ensure_repo(case: PerfCase) -> Path:
    repo = EXAMPLES_DIR / case.name
    if not (repo / ".git").exists():
        repo.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "init", "-q", str(repo)])
        run(["git", "-C", str(repo), "remote", "add", "origin", case.repo_url])
    else:
        run(["git", "-C", str(repo), "remote", "set-url", "origin", case.repo_url])

    if not has_commit(repo, f"{case.commit}^"):
        run(
            [
                "git",
                "-C",
                str(repo),
                "fetch",
                "--quiet",
                "--no-tags",
                "--depth=2",
                "origin",
                case.commit,
            ]
        )
    run(["git", "-C", str(repo), "checkout", "--quiet", "--detach", case.commit])
    return repo


def has_commit(repo: Path, commit: str) -> bool:
    completed = subprocess.run(
        ["git", "-C", str(repo), "cat-file", "-e", f"{commit}^{{commit}}"],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return completed.returncode == 0


def parse_timings(text: str) -> dict[str, str]:
    timings: dict[str, str] = {}
    for line in text.splitlines():
        match = re.match(r"  ([a-z_]+): (.+)", line)
        if match:
            timings[match.group(1)] = match.group(2)
    return timings


def git(repo: Path, *args: str) -> str:
    return run(["git", "-C", str(repo), *args]).stdout.strip()


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        sys.stderr.write(completed.stderr)
        completed.check_returncode()
    return completed


if __name__ == "__main__":
    main()
