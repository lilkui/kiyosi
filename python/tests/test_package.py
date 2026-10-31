"""Installed artifact, exports, and typing checks."""

import ast
import importlib.metadata
import inspect
import pickle
import tomllib
import unittest
from datetime import date
from pathlib import Path

import kiyosi
from kiyosi import _native, instruments, market, pricing


class PackageTests(unittest.TestCase):
    def test_terminal_quadrature_engines_are_not_exported(self):
        for name in ("QuadratureVanillaEngine", "QuadratureDigitalEngine"):
            with self.subTest(engine=name):
                self.assertFalse(hasattr(_native, name))
                self.assertFalse(hasattr(pricing, name))
                self.assertNotIn(name, pricing.__all__)

    def test_greek_methods_expose_declared_keyword_controls(self):
        stub = ast.parse((Path(kiyosi.__file__).parent / "_native.pyi").read_text())
        for node in stub.body:
            if not isinstance(node, ast.ClassDef) or not node.name.endswith("Engine"):
                continue
            method = next(
                member
                for member in node.body
                if isinstance(member, ast.FunctionDef)
                and member.name == "price_with_greeks"
            )
            expected = tuple(arg.arg for arg in method.args.args if arg.arg != "self")
            keywords = tuple(arg.arg for arg in method.args.kwonlyargs)
            engine = getattr(pricing, node.name)()
            with self.subTest(engine=node.name):
                signature = inspect.signature(engine.price_with_greeks)
                self.assertEqual(tuple(signature.parameters), expected + keywords)
                for name in keywords:
                    parameter = signature.parameters[name]
                    self.assertIs(parameter.kind, inspect.Parameter.KEYWORD_ONLY)
                    self.assertIsNot(parameter.default, inspect.Parameter.empty)

    def test_coupon_solver_exposes_keyword_controls(self):
        signature = inspect.signature(pricing.implied_coupon)
        self.assertEqual(
            tuple(signature.parameters),
            (
                "engine",
                "instrument",
                "context",
                "observed_price",
                "quote_convention",
                "lower_bound",
                "upper_bound",
                "price_tolerance",
                "parameter_tolerance",
                "max_iterations",
            ),
        )
        for name in tuple(signature.parameters)[4:]:
            self.assertIs(
                signature.parameters[name].kind, inspect.Parameter.KEYWORD_ONLY
            )

    def test_monte_carlo_spot_shift_preserves_explicit_none_rules(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        terms = {
            "option_type": "put",
            "strike": 100,
            "effective_date": start,
            "expiry_date": end,
        }
        engine = pricing.MonteCarloVanillaEngine(path_count=256, step_count=5, seed=42)
        with self.assertRaises(TypeError):
            engine.price_with_greeks(
                instruments.EuropeanOption(**terms), context, "delta", spot_shift=None
            )
        option = instruments.AmericanOption(**terms)
        default = engine.price_with_greeks(option, context, "delta")
        explicit = engine.price_with_greeks(option, context, "delta", spot_shift=None)
        self.assertEqual(explicit.require("delta"), default.require("delta"))

    def test_native_greek_methods_expose_only_request_controls(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": start,
            "expiry_date": end,
        }
        for engine, option in (
            (pricing.AnalyticEuropeanEngine(), instruments.EuropeanOption(**terms)),
            (
                pricing.MonteCarloDigitalEngine(path_count=32, seed=42),
                instruments.CashOrNothingOption(**terms, payout=10),
            ),
            *(
                (
                    pricing.MonteCarloAsianEngine(path_count=32, seed=42),
                    kind(**terms, averaging_start_date=start),
                )
                for kind in (
                    instruments.GeometricAveragePriceOption,
                    instruments.ArithmeticAveragePriceOption,
                )
            ),
        ):
            with self.subTest(
                engine=type(engine).__name__, instrument=type(option).__name__
            ):
                self.assertEqual(
                    tuple(inspect.signature(engine.price_with_greeks).parameters),
                    ("instrument", "context", "greeks", "all_greeks"),
                )
                for field in (
                    "spot_shift",
                    "volatility_shift",
                    "rate_shift",
                    "time_shift_days",
                ):
                    with self.assertRaises(TypeError):
                        engine.price_with_greeks(option, context, "delta", **{field: 1})
                for invalid in (1, None, "true"):
                    with self.assertRaises(TypeError):
                        engine.price_with_greeks(option, context, all_greeks=invalid)

    def test_constructors_and_factories_match_declared_keywords(self):
        stub = ast.parse((Path(kiyosi.__file__).parent / "_native.pyi").read_text())
        factories = [pricing.implied_volatility]
        for module in (instruments, market):
            for node in stub.body:
                if getattr(node, "name", None) not in module.__all__:
                    continue
                if isinstance(node, ast.FunctionDef) or (
                    isinstance(node, ast.ClassDef)
                    and any(
                        isinstance(member, ast.FunctionDef)
                        and member.name == "__init__"
                        for member in node.body
                    )
                ):
                    factories.append(getattr(module, node.name))
        self.assertGreater(len(factories), 30)
        for factory in factories:
            declaration = next(
                node
                for node in stub.body
                if isinstance(node, (ast.ClassDef, ast.FunctionDef))
                and node.name == factory.__name__
            )
            arguments = (
                next(
                    member
                    for member in declaration.body
                    if isinstance(member, ast.FunctionDef) and member.name == "__init__"
                ).args
                if isinstance(declaration, ast.ClassDef)
                else declaration.args
            )
            positional = tuple(arg.arg for arg in arguments.args if arg.arg != "self")
            keywords = tuple(arg.arg for arg in arguments.kwonlyargs)
            signature = inspect.signature(factory)
            with self.subTest(api=factory.__name__):
                self.assertEqual(tuple(signature.parameters), positional + keywords)
                for name in positional:
                    self.assertIs(
                        signature.parameters[name].kind,
                        inspect.Parameter.POSITIONAL_OR_KEYWORD,
                    )
                for name, default in zip(keywords, arguments.kw_defaults):
                    parameter = signature.parameters[name]
                    self.assertIs(parameter.kind, inspect.Parameter.KEYWORD_ONLY)
                    self.assertEqual(
                        parameter.default is inspect.Parameter.empty, default is None
                    )

    def test_engine_constructors_expose_keyword_only_signatures(self):
        package = Path(kiyosi.__file__).parent
        stub = ast.parse((package / "_native.pyi").read_text())
        for node in stub.body:
            if not isinstance(node, ast.ClassDef) or not node.name.endswith("Engine"):
                continue
            constructor = next(
                member
                for member in node.body
                if isinstance(member, ast.FunctionDef) and member.name == "__init__"
            )
            expected = tuple(arg.arg for arg in constructor.args.kwonlyargs)
            if not expected:
                continue
            factory = getattr(pricing, node.name)
            with self.subTest(engine=node.name):
                signature = inspect.signature(factory)
                self.assertEqual(tuple(signature.parameters), expected)
                for parameter in signature.parameters.values():
                    self.assertIs(parameter.kind, inspect.Parameter.KEYWORD_ONLY)
                    self.assertIsNot(parameter.default, inspect.Parameter.empty)
                with self.assertRaises(TypeError):
                    factory(1)
                with self.assertRaises(TypeError):
                    factory(unknown=1)

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

    def test_monte_carlo_flags_require_builtin_bools(self):
        bool_like = type(
            "bool_", (), {"__module__": "numpy", "__bool__": lambda self: True}
        )()
        for name in pricing.__all__:
            if name.startswith("MonteCarlo") and name.endswith("Engine"):
                factory = getattr(pricing, name)
                with self.subTest(engine=name):
                    self.assertTrue(factory(antithetic=True).antithetic)
                    self.assertFalse(factory(antithetic=False).antithetic)
                    for value in (1, None, bool_like):
                        with self.assertRaises(TypeError):
                            factory(antithetic=value)

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
