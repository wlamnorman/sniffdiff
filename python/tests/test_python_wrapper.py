from __future__ import annotations

import os
import sys
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from sniffdiff import SniffdiffNotFound, find_sniffdiff_bin
from sniffdiff import __main__ as sniffdiff_main
from sniffdiff import _find_sniffdiff


class FindSniffdiffBinTests(unittest.TestCase):
    def test_uses_explicit_binary_override(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            binary = Path(tmpdir) / "sniffdiff"
            binary.touch()

            with mock.patch.dict(os.environ, {"SNIFFDIFF_BIN": str(binary)}):
                self.assertEqual(find_sniffdiff_bin(), str(binary))

    def test_rejects_invalid_binary_override(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            missing = Path(tmpdir) / "sniffdiff"

            with mock.patch.dict(os.environ, {"SNIFFDIFF_BIN": str(missing)}):
                with self.assertRaises(SniffdiffNotFound):
                    find_sniffdiff_bin()

    def test_finds_binary_from_prefix_install_layout(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            prefix = Path(tmpdir)
            binary = prefix / "bin" / "sniffdiff"
            binary.parent.mkdir()
            binary.touch()
            module_path = prefix / "lib" / "python3.13" / "site-packages" / "sniffdiff"

            with ExitStack() as stack:
                stack.enter_context(mock.patch.dict(os.environ, {}, clear=True))
                stack.enter_context(
                    mock.patch.object(
                        _find_sniffdiff, "_module_path", return_value=str(module_path)
                    )
                )
                stack.enter_context(
                    mock.patch.object(_find_sniffdiff, "_sysconfig_path", return_value=None)
                )
                stack.enter_context(mock.patch.object(sys, "platform", "linux"))

                self.assertEqual(find_sniffdiff_bin(), str(binary))

    def test_finds_binary_from_target_install_layout(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            target = Path(tmpdir)
            binary = target / "bin" / "sniffdiff"
            binary.parent.mkdir()
            binary.touch()
            module_path = target / "sniffdiff"

            with ExitStack() as stack:
                stack.enter_context(mock.patch.dict(os.environ, {}, clear=True))
                stack.enter_context(
                    mock.patch.object(
                        _find_sniffdiff, "_module_path", return_value=str(module_path)
                    )
                )
                stack.enter_context(
                    mock.patch.object(_find_sniffdiff, "_sysconfig_path", return_value=None)
                )
                stack.enter_context(mock.patch.object(sys, "platform", "linux"))

                self.assertEqual(find_sniffdiff_bin(), str(binary))

    def test_does_not_fall_back_to_unrelated_path_binary(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path_bin = Path(tmpdir) / "sniffdiff"
            path_bin.touch()

            with ExitStack() as stack:
                stack.enter_context(mock.patch.dict(os.environ, {"PATH": tmpdir}, clear=True))
                stack.enter_context(
                    mock.patch.object(_find_sniffdiff, "_candidate_dirs", return_value=[])
                )

                with self.assertRaises(SniffdiffNotFound):
                    find_sniffdiff_bin()


class MainTests(unittest.TestCase):
    def test_windows_main_forwards_exit_code(self) -> None:
        completed = SimpleNamespace(returncode=7)

        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(sys, "platform", "win32"))
            stack.enter_context(mock.patch.object(sys, "argv", ["sniffdiff", "--help"]))
            stack.enter_context(
                mock.patch.object(
                    sniffdiff_main, "find_sniffdiff_bin", return_value="/bin/sniffdiff"
                )
            )
            run = stack.enter_context(
                mock.patch.object(sniffdiff_main.subprocess, "run", return_value=completed)
            )

            with self.assertRaises(SystemExit) as raised:
                sniffdiff_main.main()

        self.assertEqual(raised.exception.code, 7)
        run.assert_called_once_with(["/bin/sniffdiff", "--help"], check=False)


if __name__ == "__main__":
    unittest.main()
