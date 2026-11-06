"""Installed artifact, exports, and typing checks."""

import ast
import importlib.metadata
import inspect
import pickle
import tomllib
import unittest
from datetime import datetime
from datetime import time as clock_time
from pathlib import Path

import kiyosi
from kiyosi import _native, instruments, market, pricing
from kiyosi.pricing import numerical_greeks


class PackageTests(unittest.TestCase):
    def test_engines_compare_by_type_and_configuration(self):
        factories = [
            getattr(pricing, name)
            for name in pricing.__all__
            if name.endswith("Engine")
        ]
        for index, factory in enumerate(factories):
            engine = factory()
            with self.subTest(engine=factory.__name__):
                self.assertEqual(engine, factory())
                self.assertEqual(engine, pickle.loads(pickle.dumps(engine)))
                self.assertNotEqual(engine, factories[(index + 1) % len(factories)]())
                self.assertNotEqual(engine, object())
                with self.assertRaises(TypeError):
                    hash(engine)
                for name in inspect.signature(factory).parameters:
                    value = getattr(engine, name)
                    if value is None:
                        alternate = 42 if name == "seed" else 1000.0
                    elif isinstance(value, bool):
                        alternate = not value
                    elif isinstance(value, str):
                        alternate = {
                            "scheme": "implicit_euler",
                            "sampling": "sobol",
                        }[name]
                    else:
                        alternate = value * 2 if value else 1
                    with self.subTest(field=name):
                        self.assertNotEqual(engine, factory(**{name: alternate}))

    def test_engine_signature_defaults_can_be_forwarded(self):
        for name in pricing.__all__:
            if not name.endswith("Engine"):
                continue
            factory = getattr(pricing, name)
            arguments = inspect.signature(factory).bind()
            arguments.apply_defaults()
            original = factory()
            replay = factory(*arguments.args, **arguments.kwargs)
            if name.startswith("FiniteDifference"):
                self.assertEqual(original.time_step_count, 400)
            for field, default in arguments.arguments.items():
                with self.subTest(engine=name, field=field):
                    self.assertIsNot(default, Ellipsis)
                    self.assertEqual(getattr(original, field), default)
                    self.assertEqual(getattr(replay, field), default)

    def test_contract_schedule_and_solver_signature_defaults_can_be_forwarded(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        required = {
            "option_type": "call",
            "strike": 100,
            "effective_time": start,
            "expiry_time": end,
            "barrier_level": 120,
            "barrier_type": "up_and_out",
        }
        arguments = inspect.signature(instruments.BarrierOption).bind(**required)
        arguments.apply_defaults()
        option = instruments.BarrierOption(*arguments.args, **arguments.kwargs)
        self.assertEqual(option.rebate, 0)
        self.assertEqual(option.rebate_timing, "at_expiry")
        self.assertEqual(option.observation_mode, "continuous")
        self.assertEqual(option.observation_times, [])
        arguments = inspect.signature(market.monthly_schedule).bind(
            observation_clock_time=clock_time(),
            start_time=start,
            end_time=datetime(2025, 3, 3),
            first_observation_month=1,
        )
        arguments.apply_defaults()
        self.assertEqual(
            list(market.monthly_schedule(*arguments.args, **arguments.kwargs)),
            [datetime(2025, 2, 3), datetime(2025, 3, 3)],
        )
        option = instruments.EuropeanOption(
            option_type="call",
            strike=100,
            effective_time=start,
            expiry_time=end,
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                volatility=0.2,
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.AnalyticEuropeanEngine()
        arguments = inspect.signature(pricing.implied_volatility).bind(
            engine,
            option,
            context,
            engine.price(option, context),
        )
        arguments.apply_defaults()
        self.assertAlmostEqual(
            pricing.implied_volatility(*arguments.args, **arguments.kwargs),
            0.2,
            delta=1e-8,
        )
        digital = instruments.CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=10,
            effective_time=start,
            expiry_time=end,
        )
        engine = pricing.AnalyticDigitalEngine()
        arguments = inspect.signature(engine.price_with_greeks).bind(
            digital, context, "delta"
        )
        arguments.apply_defaults()
        result = engine.price_with_greeks(*arguments.args, **arguments.kwargs)
        self.assertEqual(
            result.delta, engine.price_with_greeks(digital, context, "delta").delta
        )

    def test_replacements_expose_declared_keyword_fields(self):
        stub = ast.parse((Path(kiyosi.__file__).parent / "_native.pyi").read_text())
        for node in stub.body:
            if not isinstance(node, ast.ClassDef):
                continue
            method = next(
                (
                    member
                    for member in node.body
                    if isinstance(member, ast.FunctionDef)
                    and member.name in ("replace", "replace_history")
                ),
                None,
            )
            if method is None:
                continue
            signature = inspect.signature(
                getattr(getattr(_native, node.name), method.name)
            )
            keywords = tuple(arg.arg for arg in method.args.kwonlyargs)
            with self.subTest(api=f"{node.name}.{method.name}"):
                self.assertEqual(tuple(signature.parameters), ("self", *keywords))
                for name, default in zip(keywords, method.args.kw_defaults):
                    parameter = signature.parameters[name]
                    self.assertIs(parameter.kind, inspect.Parameter.KEYWORD_ONLY)
                    if node.name == "PhoenixOption" and name in (
                        "coupon_rate",
                        "coupon_per_period",
                    ):
                        self.assertIs(parameter.default, Ellipsis)
                    else:
                        self.assertEqual(
                            parameter.default is inspect.Parameter.empty,
                            default is None,
                        )
                    if default is not None:
                        self.assertIs(parameter.default, Ellipsis)

    def test_replacement_signature_defaults_can_be_forwarded(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        dates = {"effective_time": start, "expiry_time": end}
        vanilla = dict(dates, option_type="call", strike=100)
        model = market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        values = [
            model,
            market.PricingContext(
                model_parameters=model, spot_price=100, valuation_time=start
            ),
            instruments.AmericanOption(**vanilla),
            instruments.CashOrNothingOption(**vanilla, payout=10),
            instruments.AssetOrNothingOption(**vanilla),
            instruments.GeometricAveragePriceOption(
                **vanilla, averaging_start_time=start, realized_average=None
            ),
            instruments.ArithmeticAveragePriceOption(
                **vanilla, averaging_start_time=start, realized_average=None
            ),
            instruments.BarrierOption(
                **vanilla,
                barrier_type="up_and_out",
                barrier_level=120,
                touch_state="untouched",
            ),
            instruments.RangeAccrualOption(
                observation_clock_time=clock_time(),
                **dates,
                notional=1000,
                annual_coupon_rate=0.12,
                lower_bound=95,
                upper_bound=110,
                realized_hit_count=3,
            ),
        ]
        for value in values:
            replace = getattr(value, "replace", None) or value.replace_history
            defaults = {
                name: parameter.default
                for name, parameter in inspect.signature(replace).parameters.items()
            }
            with self.subTest(type=type(value).__name__):
                replacement = replace(**defaults)
                self.assertEqual(replacement, value)
                self.assertIsNot(replacement, value)
        self.assertIsNone(values[-2].replace_history(touch_state=None).touch_state)
        self.assertIsNone(
            values[-1].replace_history(realized_hit_count=None).realized_hit_count
        )

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
                self.assertIs(
                    signature.parameters["greeks"].default, inspect.Parameter.empty
                )
                self.assertEqual(method.args.defaults, [])
                for name in keywords:
                    parameter = signature.parameters[name]
                    self.assertIs(parameter.kind, inspect.Parameter.KEYWORD_ONLY)
                    self.assertIsNot(parameter.default, inspect.Parameter.empty)

    def test_coupon_solver_exposes_keyword_controls(self):
        signature = inspect.signature(pricing.implied_snowball_coupon)
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
        self.assertIs(
            signature.parameters["quote_convention"].default, inspect.Parameter.empty
        )
        phoenix = inspect.signature(pricing.implied_phoenix_coupon)
        self.assertEqual(
            tuple(phoenix.parameters),
            tuple(name for name in signature.parameters if name != "quote_convention"),
        )
        for name in tuple(signature.parameters)[4:]:
            self.assertIs(
                signature.parameters[name].kind, inspect.Parameter.KEYWORD_ONLY
            )

    def test_monte_carlo_spot_shift_none_selects_each_product_default(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
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
            "effective_time": start,
            "expiry_time": end,
        }
        engine = pricing.MonteCarloVanillaEngine(path_count=256, step_count=4, seed=42)
        for kind, shift in (
            (instruments.EuropeanOption, 0.01),
            (instruments.AmericanOption, 8.0),
        ):
            option = kind(**terms)
            default = engine.price_with_greeks(option, context, "delta")
            shift_default = (
                inspect.signature(numerical_greeks).parameters["spot_shift"].default
            )
            with self.subTest(product=kind.__name__):
                self.assertIs(shift_default, None)
                self.assertEqual(
                    numerical_greeks(
                        engine, option, context, "delta", spot_shift=shift_default
                    ).require("delta"),
                    default.require("delta"),
                )
                self.assertEqual(
                    numerical_greeks(
                        engine, option, context, "delta", spot_shift=shift
                    ).require("delta"),
                    default.require("delta"),
                )
                for invalid in (True, "invalid", Ellipsis):
                    with self.assertRaises(TypeError):
                        numerical_greeks(
                            engine, option, context, "delta", spot_shift=invalid
                        )

    def test_numerical_spot_shift_none_uses_the_core_default(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        option = instruments.EuropeanOption(
            option_type="call",
            strike=100,
            effective_time=start,
            expiry_time=end,
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.QuadratureVanillaEngine()
        automatic = engine.price_with_greeks(option, context, "delta")
        explicit = numerical_greeks(engine, option, context, "delta", spot_shift=0.01)
        self.assertEqual(automatic.delta, explicit.delta)
        self.assertEqual(
            numerical_greeks(engine, option, context, "delta", spot_shift=None).delta,
            explicit.delta,
        )
        for invalid in (0, -1, float("nan"), float("inf")):
            with (
                self.subTest(invalid=invalid),
                self.assertRaises(kiyosi.InvalidInputError),
            ):
                numerical_greeks(engine, option, context, "delta", spot_shift=invalid)

    def test_native_greek_methods_reject_numerical_controls(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
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
            "effective_time": start,
            "expiry_time": end,
        }
        for engine, option in (
            (pricing.AnalyticEuropeanEngine(), instruments.EuropeanOption(**terms)),
            (
                pricing.MonteCarloDigitalEngine(path_count=32, seed=42),
                instruments.CashOrNothingOption(**terms, payout=10),
            ),
            (
                pricing.CoxRossRubinsteinDigitalEngine(),
                instruments.CashOrNothingOption(**terms, payout=10),
            ),
            (
                pricing.CoxRossRubinsteinBarrierEngine(),
                instruments.BarrierOption(
                    **terms, barrier_type="up_and_out", barrier_level=120
                ),
            ),
            *(
                (
                    pricing.MonteCarloAsianEngine(path_count=32, seed=42),
                    kind(**terms, averaging_start_time=start),
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
                    (
                        "instrument",
                        "context",
                        "greeks",
                    ),
                )
                result = numerical_greeks(
                    engine,
                    option,
                    context,
                    "delta",
                    spot_shift=0.2,
                    volatility_shift=0.001,
                    rate_shift=0.001,
                    time_shift_days=2,
                )
                self.assertEqual(result.price, engine.price(option, context))
                for control in (
                    "spot_shift",
                    "volatility_shift",
                    "rate_shift",
                    "time_shift_days",
                ):
                    with self.subTest(control=control), self.assertRaises(TypeError):
                        engine.price_with_greeks(
                            option, context, "delta", **{control: 1}
                        )
                for invalid in (1, None, True):
                    with self.assertRaises(TypeError):
                        engine.price_with_greeks(option, context, greeks=invalid)
                with self.assertRaises(TypeError):
                    engine.price_with_greeks(option, context, "delta", unknown=True)

    def test_all_engines_expose_the_same_native_greek_signature(self):
        for name in pricing.__all__:
            if not name.endswith("Engine"):
                continue
            engine = getattr(pricing, name)()
            with self.subTest(engine=name):
                for method in ("greek_method", "active_greek_controls"):
                    self.assertEqual(
                        tuple(inspect.signature(getattr(engine, method)).parameters),
                        ("instrument", "context"),
                    )
                signature = inspect.signature(engine.price_with_greeks)
                self.assertEqual(
                    tuple(signature.parameters),
                    (
                        "instrument",
                        "context",
                        "greeks",
                    ),
                )

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
        self.assertTrue(factories)
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
                    if factory is instruments.PhoenixOption and name in (
                        "coupon_rate",
                        "coupon_per_period",
                    ):
                        self.assertIs(parameter.default, Ellipsis)
                    else:
                        self.assertEqual(
                            parameter.default is inspect.Parameter.empty,
                            default is None,
                        )

    def test_autocallable_monitoring_defaults_match_explicit_daily_monitoring(self):
        start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
        common = {
            "knock_in_level": 80,
            "effective_time": start,
            "expiry_time": end,
            "observations": [instruments.AutocallableObservation(end, 110)],
        }
        for cls, extra in (
            (
                instruments.SnowballOption,
                {
                    "maturity_coupon_rate": 0.1,
                    "initial_spot": 100,
                    "upper_strike": 100,
                    "lower_strike": 0,
                },
            ),
            (
                instruments.PhoenixOption,
                {
                    "coupon_rate": 0.1,
                    "initial_spot": 100,
                    "upper_strike": 100,
                    "lower_strike": 0,
                },
            ),
            (
                instruments.TernarySnowballOption,
                {"maturity_coupon_rate": 0.1, "minimum_coupon_rate": 0.02},
            ),
        ):
            with self.subTest(contract=cls.__name__):
                note = cls(observation_clock_time=clock_time(), **common, **extra)
                self.assertEqual(note.knock_in_observation_mode, "every_trading_day")
                default = (
                    inspect.signature(cls)
                    .parameters["knock_in_observation_mode"]
                    .default
                )
                self.assertEqual(default, "every_trading_day")
                self.assertEqual(
                    note,
                    cls(
                        observation_clock_time=clock_time(),
                        **common,
                        **extra,
                        knock_in_observation_mode=default,
                    ),
                )
                self.assertNotEqual(
                    note, cls(**common, **extra, knock_in_observation_mode="at_expiry")
                )
                for invalid in (None, True, "invalid"):
                    with self.assertRaises((TypeError, ValueError)):
                        cls(**common, **extra, knock_in_observation_mode=invalid)

    def test_binary_and_touch_constructors_require_explicit_terms(self):
        for cls in (
            instruments.BinaryBarrierOption,
            instruments.TouchOption,
            instruments.DoubleTouchOption,
        ):
            with self.subTest(contract=cls.__name__):
                signature = inspect.signature(cls)
                self.assertIs(
                    signature.parameters[
                        "payout"
                        if cls is instruments.DoubleTouchOption
                        else "payoff_type"
                    ].default,
                    inspect.Parameter.empty,
                )
                self.assertTrue(
                    all(
                        p.kind is inspect.Parameter.KEYWORD_ONLY
                        for p in signature.parameters.values()
                    )
                )
                with self.assertRaises(TypeError):
                    signature.bind()
                with self.assertRaises(TypeError):
                    cls()

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
        from datetime import datetime

        from kiyosi.instruments import EuropeanOption

        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_time": datetime(2025, 1, 1),
            "expiry_time": datetime(2026, 1, 1),
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
                    "MonteCarloUncertaintyStatus",
                    "CouponQuoteConvention",
                    "GreekMethod",
                    "GreekControl",
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
