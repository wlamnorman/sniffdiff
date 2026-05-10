from __future__ import annotations

from fnmatch import fnmatch
import os
import sys
import sysconfig
from pathlib import Path
from typing import Callable, Optional, cast


class SniffdiffNotFound(FileNotFoundError):
    pass


def find_sniffdiff_bin() -> str:
    """Return the installed sniffdiff binary path."""

    if override := os.environ.get("SNIFFDIFF_BIN"):
        path = Path(override)
        if path.is_file():
            return str(path)
        raise SniffdiffNotFound(f"SNIFFDIFF_BIN does not point to a file: {path}")

    sniffdiff_exe = "sniffdiff" + (sysconfig.get_config_var("EXE") or "")
    candidates = _candidate_dirs()

    for directory in candidates:
        path = os.path.join(directory, sniffdiff_exe)
        if os.path.isfile(path):
            return path

    locations = "\n".join(f" - {directory}" for directory in candidates)
    raise SniffdiffNotFound(
        f"Could not find the sniffdiff binary in any of these locations:\n{locations}"
    )


def _candidate_dirs() -> list[str]:
    targets = [
        _sysconfig_path("scripts"),
        _sysconfig_path("scripts", vars={"base": sys.base_prefix}),
        (
            _join(
                _matching_parents(_module_path(), "Lib/site-packages/sniffdiff"),
                "Scripts",
            )
            if sys.platform == "win32"
            else _join(
                _matching_parents(_module_path(), "lib/python*/site-packages/sniffdiff"),
                "bin",
            )
        ),
        _join(_matching_parents(_module_path(), "sniffdiff"), "bin"),
        _sysconfig_path("scripts", scheme=_user_scheme()),
    ]

    seen: list[str] = []
    for target in targets:
        if not target:
            continue
        if target in seen:
            continue
        seen.append(target)
    return seen


def _sysconfig_path(
    name: str,
    *,
    scheme: Optional[str] = None,
    vars: Optional[dict[str, str]] = None,
) -> Optional[str]:
    try:
        if scheme is None:
            return sysconfig.get_path(name, vars=vars)
        return sysconfig.get_path(name, scheme=scheme, vars=vars)
    except (KeyError, TypeError):
        return None


def _module_path() -> Optional[str]:
    return os.path.dirname(__file__)


def _matching_parents(path: Optional[str], match: str) -> Optional[str]:
    """Return the parent directory of path after trimming a matching suffix."""
    if not path:
        return None

    parts = path.split(os.sep)
    match_parts = match.split("/")
    if len(parts) < len(match_parts):
        return None

    if not all(
        fnmatch(part, match_part)
        for part, match_part in zip(reversed(parts), reversed(match_parts))
    ):
        return None

    return os.sep.join(parts[: -len(match_parts)])


def _join(path: Optional[str], *parts: str) -> Optional[str]:
    if not path:
        return None
    return os.path.join(path, *parts)


def _user_scheme() -> str:
    get_preferred_scheme = getattr(sysconfig, "get_preferred_scheme", None)
    if get_preferred_scheme is not None:
        return cast(Callable[[str], str], get_preferred_scheme)("user")
    if os.name == "nt":
        return "nt_user"
    if sys.platform == "darwin" and getattr(sys, "_framework", None):
        return "osx_framework_user"
    return "posix_user"
