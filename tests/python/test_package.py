"""Installed artifact, exports, and typing checks."""

import ast
import importlib.metadata
import tomllib
import unittest
from enum import Enum
from pathlib import Path

import kiyosi
from kiyosi import _native, instruments, market, pricing


class PackageTests(unittest.TestCase):
    def test_error_categories_are_named_enum_members(self):
        for category in kiyosi.ErrorCategory:
            with self.subTest(category=category.name):
                self.assertIsInstance(category, Enum)
                self.assertNotIsInstance(category, int)
                self.assertEqual(category.value, category.name)
                self.assertNotEqual(category, category.value)
                self.assertTrue(category)
                self.assertIs(kiyosi.ErrorCategory(category.name), category)

    def test_installed_version_and_typed_exports(self):
        workspace = Path(__file__).resolve().parents[2]
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
                if name not in (
                    "__version__",
                    "OptionType",
                    "BusinessDayConvention",
                    "Greek",
                    "FiniteDifferenceScheme",
                    "MonteCarloSampling",
                    "CouponQuoteConvention",
                ):
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
            member.targets[0].id
            for member in categories.body
            if isinstance(member, ast.Assign)
        }
        self.assertEqual(expected, set(kiyosi.ErrorCategory.__members__))
