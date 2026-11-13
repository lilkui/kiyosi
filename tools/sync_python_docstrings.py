"""Copy native API documentation into the type stub for editor discovery.

Notes
-----
Signatures are maintained in the stub; docstrings come from the Rust bindings.

Examples
--------
Rebuild the extension in an editable install, then synchronize its docs::

    python tools/sync_python_docstrings.py
    ruff check --fix python/src/kiyosi/_native.pyi
    ruff format python/src/kiyosi/_native.pyi
"""

import ast
import inspect
from pathlib import Path

from kiyosi import _native

STUB = Path(__file__).resolve().parents[1] / "python/src/kiyosi/_native.pyi"
_SHARED_OWNERS = {
    "_PricingMethods": _native.BjerksundStenslandVanillaEngine,
    "_FiniteDifferenceMethods": _native.FiniteDifferenceVanillaEngine,
    "_MonteCarloMethods": _native.MonteCarloBarrierEngine,
}


def sync_docstrings(declarations, owner):
    documented = set()
    for node in declarations:
        if not isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            continue
        if isinstance(node, ast.ClassDef) and node.name in _SHARED_OWNERS:
            sync_docstrings(node.body, _SHARED_OWNERS[node.name])
            continue
        if node.name.startswith("_"):
            continue
        if node.name in documented:
            if ast.get_docstring(node) is not None:
                node.body.pop(0)
            if not node.body:
                node.body.append(ast.Expr(value=ast.Constant(value=Ellipsis)))
            continue
        documented.add(node.name)
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
            node.body.insert(
                0,
                ast.Expr(value=ast.Constant(value=doc + "\n" if "\n" in doc else doc)),
            )
        if isinstance(node, ast.ClassDef):
            sync_docstrings(node.body, value)


if __name__ == "__main__":
    stub = ast.parse(STUB.read_text(encoding="utf-8"))
    sync_docstrings(stub.body, _native)
    source = ast.unparse(stub) + "\n"
    STUB.write_text(source, encoding="utf-8")
