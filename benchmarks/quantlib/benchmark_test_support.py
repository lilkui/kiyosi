"""Executable fixtures for the Linux benchmark runner."""

import shlex
import sys
from pathlib import Path


def write_python_adapter(executable: Path, source: str) -> None:
    script = executable.with_suffix(".py")
    script.write_text(source, encoding="utf-8")
    command = shlex.join([sys.executable, str(script)])
    executable.write_text(f'#!/bin/sh\nexec {command} "$@"\n', encoding="utf-8")
    executable.chmod(0o755)
