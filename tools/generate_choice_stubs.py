"""Preserve string Literal values in nanobind's generated alias declarations."""

import importlib
import sys
from pathlib import Path
from typing import Literal, get_args, get_origin


def generate(native_directory: str, output: str) -> None:
    sys.path.insert(0, native_directory)
    module = importlib.import_module("_native")
    patterns = []
    for name, alias in vars(module).items():
        if get_origin(alias) is Literal:
            choices = ", ".join(repr(choice) for choice in get_args(alias))
            patterns.append(
                rf"_native\.{name}:" + "\n"
                "    \\from typing import Literal, TypeAlias\n"
                f"    {name}: TypeAlias = Literal[{choices}]\n"
            )
    Path(output).write_text("\n".join(patterns), encoding="utf-8")


if __name__ == "__main__":
    generate(*sys.argv[1:])
