"""Installed artifact, exports, and typing checks."""

import ast
import importlib.metadata
import pickle
import tomllib
import unittest
from pathlib import Path

import kiyosi
from kiyosi import _native, instruments, market, pricing


class PackageTests(unittest.TestCase):
    def test_domain_exception_hierarchy_and_diagnostics(self):
        self.assertEqual(kiyosi.KiyosiError.__bases__, (Exception,))
        for name in (
            "InvalidInputError",
            "UnsupportedOperationError",
            "ComputationFailureError",
            "RootNotBracketedError",
            "NonConvergenceError",
        ):
            with self.subTest(exception=name):
                exception_type = getattr(kiyosi, name)
                self.assertIs(exception_type, getattr(_native, name))
                self.assertEqual(exception_type.__bases__, (kiyosi.KiyosiError,))
                self.assertEqual(exception_type.__module__, "kiyosi._native")
                error = exception_type("diagnostic details")
                self.assertEqual(str(error), "diagnostic details")
                self.assertEqual(error.args, ("diagnostic details",))
                restored = pickle.loads(pickle.dumps(error))
                self.assertIs(type(restored), exception_type)
                self.assertEqual(restored.args, error.args)

    def test_domain_rejections_preserve_native_conversion_exceptions(self):
        from datetime import date

        from kiyosi.instruments import EuropeanOption

        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        with self.assertRaises(kiyosi.InvalidInputError) as rejection:
            EuropeanOption(**(terms | {"strike": -1}))
        self.assertIs(type(rejection.exception), kiyosi.InvalidInputError)
        self.assertIsInstance(rejection.exception, kiyosi.KiyosiError)
        for fields, exception_type in (
            ({"strike": True}, TypeError),
            ({"option_type": "unknown"}, ValueError),
        ):
            with (
                self.subTest(fields=fields),
                self.assertRaises(exception_type) as error,
            ):
                EuropeanOption(**(terms | fields))
            self.assertNotIsInstance(error.exception, kiyosi.KiyosiError)

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
        for name in (
            "InvalidInputError",
            "UnsupportedOperationError",
            "ComputationFailureError",
            "RootNotBracketedError",
            "NonConvergenceError",
        ):
            self.assertIn(name, kiyosi.__all__)
            self.assertEqual(
                [ast.unparse(base) for base in declarations[name].bases],
                ["KiyosiError"],
            )
