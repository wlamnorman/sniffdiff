from __future__ import annotations

import os
import subprocess
import sys

from sniffdiff import find_sniffdiff_bin


def main() -> None:
    sniffdiff = find_sniffdiff_bin()
    args = [sniffdiff, *sys.argv[1:]]

    if sys.platform == "win32":
        try:
            completed = subprocess.run(args, check=False)
        except KeyboardInterrupt:
            raise SystemExit(2) from None
        raise SystemExit(completed.returncode)

    os.execvp(sniffdiff, args)


if __name__ == "__main__":
    main()
