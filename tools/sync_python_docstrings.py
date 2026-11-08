"""Copy native API documentation into the type stub for editor discovery.

After rebuilding the extension in an editable install, run::

    python tools/sync_python_docstrings.py
    ruff check --fix python/src/kiyosi/_native.pyi
    ruff format python/src/kiyosi/_native.pyi

Signatures are maintained in the stub; docstrings come from the Rust bindings.
"""

import ast
import inspect
import io
import tokenize
from pathlib import Path

from kiyosi import _native

STUB = Path(__file__).resolve().parents[1] / "python/src/kiyosi/_native.pyi"


def sync_docstrings(declarations, owner):
    for node in declarations:
        if not isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            continue
        if node.name.startswith("_"):
            continue
        value = getattr(owner, node.name)
        doc = inspect.getdoc(value)
        if doc:
            if ast.get_docstring(node) is not None:
                node.body.pop(0)
            if (
                len(node.body) == 1
                and isinstance(node.body[0], ast.Expr)
                and isinstance(node.body[0].value, ast.Constant)
                and node.body[0].value.value is Ellipsis
            ):
                node.body.clear()
            node.body.insert(0, ast.Expr(value=ast.Constant(value=doc)))
        if isinstance(node, ast.ClassDef):
            sync_docstrings(node.body, value)


if __name__ == "__main__":
    stub = ast.parse(STUB.read_text(encoding="utf-8"))
    sync_docstrings(stub.body, _native)
    source = ast.unparse(stub) + "\n"
    for token in tokenize.generate_tokens(io.StringIO(source).readline):
        if token.type == tokenize.STRING and "\n" in token.string:
            source = source.replace(
                token.string,
                token.string[:-3] + "\n" + " " * token.start[1] + token.string[-3:],
                1,
            )
    STUB.write_text(source, encoding="utf-8")
