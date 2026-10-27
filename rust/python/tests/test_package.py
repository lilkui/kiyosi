"""Candidate-only installed artifact, exports, and typing checks."""

import ast
import importlib.metadata
import tomllib
import unittest
from pathlib import Path

import kiyosi
from kiyosi import _native, instruments, market, pricing


class PackageTests(unittest.TestCase):
    def test_installed_version_and_typed_exports(self):
        workspace = Path(__file__).resolve().parents[3]
        version = tomllib.loads((workspace / "Cargo.toml").read_text())["workspace"][
            "package"
        ]["version"]
        self.assertEqual(kiyosi.__version__, version)
        self.assertEqual(importlib.metadata.version("kiyosi"), version)
        package = Path(kiyosi.__file__).parent
        self.assertTrue((package / "py.typed").is_file())
        stub = ast.parse((package / "_native.pyi").read_text())
        declarations = {
            node.name: node
            for node in stub.body
            if isinstance(node, (ast.ClassDef, ast.FunctionDef))
        }
        for module in (kiyosi, instruments, market, pricing):
            for name in module.__all__:
                self.assertTrue(hasattr(module, name), name)
                if name not in ("__version__", "OptionType"):
                    self.assertIn(name, declarations)
                    self.assertIs(getattr(module, name), getattr(_native, name))
        for name, node in declarations.items():
            if isinstance(node, ast.ClassDef):
                runtime = getattr(_native, name)
                for member in node.body:
                    if isinstance(member, ast.FunctionDef):
                        self.assertTrue(
                            hasattr(runtime, member.name), f"{name}.{member.name}"
                        )
        categories = declarations["ErrorCategory"]
        expected = {
            member.targets[0].id: ast.literal_eval(member.value)
            for member in categories.body
            if isinstance(member, ast.Assign)
        }
        self.assertEqual(
            expected, {item.name: item.value for item in kiyosi.ErrorCategory}
        )
