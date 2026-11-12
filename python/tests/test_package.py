"""Installed artifact, exports, and typing checks."""

import ast
import importlib.metadata
import inspect
import pickle
import tomllib
from datetime import datetime, timedelta
from datetime import time as clock_time
from pathlib import Path

import kiyosi
import pytest
from kiyosi import _native, instruments, market, pricing
from kiyosi.instruments import KnockInObservation
from kiyosi.pricing import numerical_greeks


def test_engine_replacement_preserves_controls_and_validates_inputs(subtests):
    for name in pricing.__all__:
        if not name.endswith("Engine"):
            continue
        factory = getattr(pricing, name)
        original = factory()
        controls = {}
        for field in inspect.signature(factory).parameters:
            value = getattr(original, field)
            if field == "time_shift":
                alternate = timedelta(seconds=1)
            elif field == "seed":
                alternate = 42
            elif value is None:
                alternate = 1000.0
            elif isinstance(value, bool):
                alternate = not value
            elif isinstance(value, str):
                alternate = {"scheme": "implicit_euler", "sampling": "sobol"}[field]
            else:
                alternate = value * 2 if value else 1
            controls[field] = alternate
        configured = factory(**controls)
        with subtests.test(engine=name):
            assert configured.replace() == configured
            with pytest.raises(TypeError):
                configured.replace(unknown=Ellipsis)
            if not controls:
                continue
            field = next(iter(controls))
            assert configured.replace(**{field: Ellipsis}) == configured
            updated = configured.replace(**{field: getattr(original, field)})
            expected = controls | {field: getattr(original, field)}
            assert updated == factory(**expected)
            assert configured == factory(**controls)
            with pytest.raises(TypeError):
                configured.replace(**{field: None})
            with pytest.raises(kiyosi.InvalidInputError):
                configured.replace(**{field: -1})
            assert pickle.loads(pickle.dumps(updated)) == updated
            for nullable in controls:
                if getattr(original, nullable) is None:
                    cleared = configured.replace(**{nullable: None})
                    assert cleared == factory(**controls | {nullable: None})


def test_engines_compare_by_type_and_configuration(subtests):
    factories = [
        getattr(pricing, name) for name in pricing.__all__ if name.endswith("Engine")
    ]
    for index, factory in enumerate(factories):
        engine = factory()
        with subtests.test(engine=factory.__name__):
            assert engine == factory()
            assert engine == pickle.loads(pickle.dumps(engine))
            assert engine != factories[(index + 1) % len(factories)]()
            assert engine != object()
            with pytest.raises(TypeError):
                hash(engine)
            for name in inspect.signature(factory).parameters:
                value = getattr(engine, name)
                if name == "time_shift":
                    alternate = timedelta(seconds=1)
                elif value is None:
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
                with subtests.test(field=name):
                    assert engine != factory(**{name: alternate})


def test_engine_signature_defaults_can_be_forwarded(subtests):
    for name in pricing.__all__:
        if not name.endswith("Engine"):
            continue
        factory = getattr(pricing, name)
        arguments = inspect.signature(factory).bind()
        arguments.apply_defaults()
        original = factory()
        replay = factory(*arguments.args, **arguments.kwargs)
        if name.startswith("FiniteDifference"):
            assert original.time_step_count == 400
        for field, default in arguments.arguments.items():
            with subtests.test(engine=name, field=field):
                assert default is not Ellipsis
                assert getattr(original, field) == default
                assert getattr(replay, field) == default


def test_contract_schedule_and_solver_signature_defaults_can_be_forwarded():
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
    assert option.rebate == 0
    assert option.rebate_timing == "at_expiry"
    assert option.observation_mode == "continuous"
    assert option.observation_times is None
    arguments = inspect.signature(market.monthly_schedule).bind(
        observation_clock_time=clock_time(),
        start_time=start,
        end_time=datetime(2025, 3, 3),
        first_observation_month=1,
    )
    arguments.apply_defaults()
    assert list(market.monthly_schedule(*arguments.args, **arguments.kwargs)) == [
        datetime(2025, 2, 3),
        datetime(2025, 3, 3),
    ]
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
        lower_bound=0.0,
        upper_bound=4.0,
    )
    arguments.apply_defaults()
    assert pricing.implied_volatility(
        *arguments.args, **arguments.kwargs
    ) == pytest.approx(0.2, rel=0, abs=1e-8)
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
    assert result.delta == engine.price_with_greeks(digital, context, "delta").delta


def test_replacements_expose_declared_keyword_fields(subtests):
    stub = ast.parse(
        (Path(kiyosi.__file__).parent / "_native.pyi").read_text(encoding="utf-8")
    )
    for node in stub.body:
        if not isinstance(node, ast.ClassDef):
            continue
        method = next(
            (
                member
                for member in node.body
                if isinstance(member, ast.FunctionDef) and member.name == "replace"
            ),
            None,
        )
        if method is None:
            continue
        signature = inspect.signature(getattr(getattr(_native, node.name), method.name))
        keywords = tuple(arg.arg for arg in method.args.kwonlyargs)
        with subtests.test(api=f"{node.name}.{method.name}"):
            assert tuple(signature.parameters) == (("self", *keywords))
            for name, default in zip(keywords, method.args.kw_defaults):
                parameter = signature.parameters[name]
                assert parameter.kind is inspect.Parameter.KEYWORD_ONLY
                assert (parameter.default is inspect.Parameter.empty) == (
                    default is None
                )
                if default is not None:
                    assert parameter.default is Ellipsis


def test_replacement_signature_defaults_can_be_forwarded(subtests):
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
        replace = value.replace
        defaults = {
            name: parameter.default
            for name, parameter in inspect.signature(replace).parameters.items()
        }
        with subtests.test(type=type(value).__name__):
            replacement = replace(**defaults)
            assert replacement == value
            assert replacement is not value
    assert values[-2].replace(touch_state=None).touch_state is None
    assert values[-1].replace(realized_hit_count=None).realized_hit_count is None


def test_greek_methods_expose_declared_keyword_controls(subtests):
    stub = ast.parse(
        (Path(kiyosi.__file__).parent / "_native.pyi").read_text(encoding="utf-8")
    )
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
        with subtests.test(engine=node.name):
            signature = inspect.signature(engine.price_with_greeks)
            assert tuple(signature.parameters) == expected + keywords
            assert signature.parameters["greeks"].default is inspect.Parameter.empty
            assert method.args.defaults == []
            for name in keywords:
                parameter = signature.parameters[name]
                assert parameter.kind is inspect.Parameter.KEYWORD_ONLY
                assert parameter.default is not inspect.Parameter.empty


def test_coupon_solver_exposes_keyword_controls():
    signature = inspect.signature(pricing.implied_snowball_coupon)
    assert tuple(signature.parameters) == (
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
        )
    )
    assert signature.parameters["quote_convention"].default is inspect.Parameter.empty
    phoenix = inspect.signature(pricing.implied_phoenix_coupon)
    assert tuple(phoenix.parameters) == tuple(
        name for name in signature.parameters if name != "quote_convention"
    )
    for name in tuple(signature.parameters)[4:]:
        assert signature.parameters[name].kind is inspect.Parameter.KEYWORD_ONLY


@pytest.mark.parametrize(
    "kind,shift",
    ((instruments.EuropeanOption, 0.01), (instruments.AmericanOption, 8.0)),
)
def test_monte_carlo_spot_shift_none_selects_each_product_default(kind, shift):
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
    option = kind(**terms)
    default = engine.price_with_greeks(option, context, "delta")
    shift_default = inspect.signature(numerical_greeks).parameters["spot_shift"].default
    assert shift_default is Ellipsis
    assert numerical_greeks(
        engine,
        option,
        context,
        "delta",
        spot_shift=shift_default,
    ).require("delta") == default.require("delta")
    assert numerical_greeks(
        engine,
        option,
        context,
        "delta",
        spot_shift=shift,
    ).require("delta") == default.require("delta")
    for invalid in (True, "invalid"):
        with pytest.raises(TypeError):
            numerical_greeks(
                engine,
                option,
                context,
                "delta",
                spot_shift=invalid,
            )


def test_numerical_spot_shift_none_uses_the_core_default(subtests):
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
    assert automatic.delta == explicit.delta
    assert (
        numerical_greeks(
            engine,
            option,
            context,
            "delta",
            spot_shift=None,
        ).delta
        == explicit.delta
    )
    for invalid in (0, -1, float("nan"), float("inf")):
        with (
            subtests.test(invalid=invalid),
            pytest.raises(kiyosi.InvalidInputError),
        ):
            numerical_greeks(
                engine,
                option,
                context,
                "delta",
                spot_shift=invalid,
            )


def test_native_greek_methods_accept_uniform_controls(subtests):
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
        with subtests.test(
            engine=type(engine).__name__, instrument=type(option).__name__
        ):
            result = numerical_greeks(
                engine,
                option,
                context,
                "delta",
                spot_shift=0.2,
                volatility_shift=0.001,
                rate_shift=0.001,
                time_shift=timedelta(days=2),
            )
            assert result.price == engine.price(option, context)
            native = engine.price_with_greeks(
                option,
                context,
                "delta",
                spot_shift=0.2,
                volatility_shift=0.001,
                rate_shift=0.001,
                time_shift=timedelta(days=2),
            )
            assert native.price == result.price
            for control in (
                "spot_shift",
                "volatility_shift",
                "rate_shift",
                "time_shift",
            ):
                with subtests.test(control=control), pytest.raises(TypeError):
                    engine.price_with_greeks(
                        option,
                        context,
                        "delta",
                        **{control: True},
                    )
            for control, value in (
                ("spot_shift", 0),
                ("volatility_shift", float("nan")),
                ("rate_shift", 0),
                ("time_shift", timedelta()),
            ):
                with (
                    subtests.test(control=control),
                    pytest.raises(kiyosi.InvalidInputError),
                ):
                    engine.price_with_greeks(
                        option,
                        context,
                        "delta",
                        **{control: value},
                    )
            for invalid in (1, None, True):
                with pytest.raises(TypeError):
                    engine.price_with_greeks(option, context, greeks=invalid)
            with pytest.raises(TypeError):
                engine.price_with_greeks(option, context, "delta", unknown=True)


def test_native_greek_signatures_share_keyword_controls(subtests):
    for name in pricing.__all__:
        if not name.endswith("Engine"):
            continue
        engine = getattr(pricing, name)()
        with subtests.test(engine=name):
            for method in ("greek_method", "active_greek_controls"):
                assert tuple(inspect.signature(getattr(engine, method)).parameters) == (
                    ("instrument", "context")
                )
            signature = inspect.signature(engine.price_with_greeks)
            assert tuple(signature.parameters) == (
                (
                    "instrument",
                    "context",
                    "greeks",
                    "spot_shift",
                    "volatility_shift",
                    "rate_shift",
                    "time_shift",
                )
            )


def test_constructors_and_factories_match_declared_keywords(subtests):
    stub = ast.parse(
        (Path(kiyosi.__file__).parent / "_native.pyi").read_text(encoding="utf-8")
    )
    factories = [pricing.implied_volatility]
    for module in (instruments, market):
        for node in stub.body:
            if getattr(node, "name", None) not in module.__all__:
                continue
            if isinstance(node, ast.FunctionDef) or (
                isinstance(node, ast.ClassDef)
                and any(
                    isinstance(member, ast.FunctionDef) and member.name == "__init__"
                    for member in node.body
                )
            ):
                factories.append(getattr(module, node.name))
    assert factories
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
        with subtests.test(api=factory.__name__):
            assert tuple(signature.parameters) == positional + keywords
            for name in positional:
                assert (
                    signature.parameters[name].kind
                    is inspect.Parameter.POSITIONAL_OR_KEYWORD
                )
            for name, default in zip(keywords, arguments.kw_defaults):
                parameter = signature.parameters[name]
                assert parameter.kind is inspect.Parameter.KEYWORD_ONLY
                if factory is instruments.autocallable_observations and name in (
                    "knock_out_coupon_rates",
                    "coupon_barrier_levels",
                ):
                    assert parameter.default is None
                elif (
                    factory
                    in (
                        instruments.BinaryBarrierOption,
                        instruments.TouchOption,
                    )
                    and name == "payout"
                ):
                    assert parameter.default is None
                    assert default is None
                else:
                    assert (parameter.default is inspect.Parameter.empty) == (
                        default is None
                    )


@pytest.mark.parametrize(
    "cls,extra",
    (
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
                "coupon_per_period": 0.1,
                "initial_spot": 100,
                "upper_strike": 100,
                "lower_strike": 0,
            },
        ),
        (
            instruments.TernarySnowballOption,
            {"maturity_coupon_rate": 0.1, "minimum_coupon_rate": 0.02},
        ),
    ),
)
def test_autocallable_clocks_follow_monitoring_mode(cls, extra):
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    common = {
        "calendar": market.weekdays_calendar(),
        "knock_in_level": 80,
        "effective_time": start,
        "expiry_time": end,
        "autocall_observations": [
            instruments.AutocallableObservation.snowball(end, 110, coupon_rate=0)
        ],
    }
    common["autocall_observations"] = [
        instruments.AutocallableObservation.phoenix(end, 110, coupon_barrier_level=0)
        if cls is instruments.PhoenixOption
        else instruments.AutocallableObservation.snowball(end, 110, coupon_rate=0)
    ]
    signature = inspect.signature(cls)
    assert (
        signature.parameters["knock_in_observation"].default is inspect.Parameter.empty
    )
    assert "observation_clock_time" not in signature.parameters
    with pytest.raises(TypeError):
        cls(**common, **extra)
    observation = KnockInObservation.every_trading_day(clock_time())
    arguments = signature.bind(**common, **extra, knock_in_observation=observation)
    arguments.apply_defaults()
    note = cls(*arguments.args, **arguments.kwargs)
    assert note.knock_in_observation == observation
    expiry_only = cls(
        **common, **extra, knock_in_observation=KnockInObservation.at_expiry()
    )
    assert note != expiry_only
    assert list(expiry_only.knock_in_observation_schedule) == [end]
    assert pickle.loads(pickle.dumps(expiry_only)) == expiry_only
    for invalid in (None, True, "every_trading_day", "at_expiry", clock_time()):
        with pytest.raises(TypeError):
            cls(**common, **extra, knock_in_observation=invalid)


@pytest.mark.parametrize(
    "cls",
    (
        instruments.BinaryBarrierOption,
        instruments.TouchOption,
        instruments.DoubleTouchOption,
    ),
)
def test_binary_and_touch_constructors_require_explicit_terms(cls):
    signature = inspect.signature(cls)
    assert (
        signature.parameters[
            "payout" if cls is instruments.DoubleTouchOption else "payoff_type"
        ].default
        is inspect.Parameter.empty
    )
    assert all(
        p.kind is inspect.Parameter.KEYWORD_ONLY for p in signature.parameters.values()
    )
    with pytest.raises(TypeError):
        signature.bind()
    with pytest.raises(TypeError):
        cls()


def test_engine_constructors_expose_keyword_only_signatures(subtests):
    package = Path(kiyosi.__file__).parent
    stub = ast.parse((package / "_native.pyi").read_text(encoding="utf-8"))
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
        with subtests.test(engine=node.name):
            signature = inspect.signature(factory)
            assert tuple(signature.parameters) == expected
            for parameter in signature.parameters.values():
                assert parameter.kind is inspect.Parameter.KEYWORD_ONLY
                assert parameter.default is not inspect.Parameter.empty
            with pytest.raises(TypeError):
                factory(1)
            with pytest.raises(TypeError):
                factory(unknown=1)


def test_domain_exception_hierarchy_and_diagnostics(subtests):
    assert kiyosi.KiyosiError.__bases__ == ((Exception,))
    for name in (
        "InvalidInputError",
        "MissingHistoryError",
        "UnsupportedOperationError",
        "ComputationFailureError",
        "RootNotBracketedError",
        "NonConvergenceError",
    ):
        with subtests.test(exception=name):
            exception_type = getattr(kiyosi, name)
            assert exception_type is getattr(_native, name)
            assert exception_type.__bases__ == (
                (kiyosi.KiyosiError, ValueError)
                if name == "InvalidInputError"
                else (kiyosi.InvalidInputError,)
                if name == "MissingHistoryError"
                else (kiyosi.KiyosiError,)
            )
            assert exception_type.__module__ == "kiyosi._native"
            error = exception_type("diagnostic details")
            assert str(error) == "diagnostic details"
            assert error.args == (("diagnostic details",))
            restored = pickle.loads(pickle.dumps(error))
            assert type(restored) is exception_type
            assert restored.args == error.args


@pytest.mark.parametrize("name", pricing.__all__)
def test_monte_carlo_flags_require_builtin_bools(name):
    bool_like = type(
        "bool_", (), {"__module__": "numpy", "__bool__": lambda self: True}
    )()
    if name.startswith("MonteCarlo") and name.endswith("Engine"):
        factory = getattr(pricing, name)
        assert factory(antithetic=True).antithetic
        assert not (factory(antithetic=False).antithetic)
        for value in (1, None, bool_like):
            with pytest.raises(TypeError):
                factory(antithetic=value)


def test_domain_rejections_preserve_native_conversion_exceptions(subtests):
    from datetime import datetime

    from kiyosi.instruments import EuropeanOption

    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
    }
    with pytest.raises(ValueError) as rejection:
        EuropeanOption(**(terms | {"strike": -1}))
    assert type(rejection.value) is kiyosi.InvalidInputError
    assert isinstance(rejection.value, kiyosi.KiyosiError)
    for fields, exception_type in (
        ({"strike": True}, TypeError),
        ({"option_type": "unknown"}, ValueError),
    ):
        with (
            subtests.test(fields=fields),
            pytest.raises(exception_type) as error,
        ):
            EuropeanOption(**(terms | fields))
        assert not isinstance(error.value, kiyosi.KiyosiError)


def test_stub_docstrings_match_native_api(subtests):
    package = Path(kiyosi.__file__).parent
    stub = ast.parse((package / "_native.pyi").read_text(encoding="utf-8"))
    documented = set()
    for node in stub.body:
        if not isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            continue
        if node.name.startswith("_"):
            continue
        runtime = getattr(_native, node.name)
        declarations = [(node.name, node, runtime)]
        if isinstance(node, ast.ClassDef):
            declarations.extend(
                (
                    f"{node.name}.{member.name}",
                    member,
                    getattr(runtime, member.name),
                )
                for member in node.body
                if isinstance(member, ast.FunctionDef)
                and not member.name.startswith("_")
            )
        for name, declaration, value in declarations:
            with subtests.test(symbol=name, line=declaration.lineno):
                if name in documented:
                    assert ast.get_docstring(declaration) is None
                    continue
                documented.add(name)
                assert ast.get_docstring(declaration) is not None
                assert ast.get_docstring(declaration) == inspect.getdoc(value), (
                    "Rebuild the extension and run tools/sync_python_docstrings.py"
                )


def test_installed_version_and_typed_exports():
    workspace = Path(__file__).resolve().parents[2]
    version = tomllib.loads((workspace / "Cargo.toml").read_text())["workspace"][
        "package"
    ]["version"]
    assert kiyosi.__version__ == version
    assert importlib.metadata.version("kiyosi") == version
    package = Path(kiyosi.__file__).parent
    assert (package / "py.typed").is_file()
    stub = ast.parse((package / "_native.pyi").read_text(encoding="utf-8"))
    declarations = {
        node.name: node
        for node in stub.body
        if isinstance(node, (ast.ClassDef, ast.FunctionDef))
    }
    for module in (kiyosi, instruments, market, pricing):
        for name in module.__all__:
            assert hasattr(module, name), name
            if name not in (
                "__version__",
                "OptionType",
                "BusinessDayConvention",
                "Greek",
                "FiniteDifferenceScheme",
                "MonteCarloSampling",
                "MonteCarloUncertaintyStatus",
                "CouponQuoteConvention",
                "PhoenixCouponQuote",
                "SnowballCouponQuote",
                "GreekMethod",
                "GreekControl",
            ):
                assert name in declarations
                assert getattr(module, name) is getattr(_native, name)
    for name, node in declarations.items():
        if isinstance(node, ast.ClassDef):
            runtime = getattr(_native, name)
            for member in node.body:
                if isinstance(member, ast.FunctionDef):
                    assert hasattr(runtime, member.name), f"{name}.{member.name}"
    for name in (
        "InvalidInputError",
        "MissingHistoryError",
        "UnsupportedOperationError",
        "ComputationFailureError",
        "RootNotBracketedError",
        "NonConvergenceError",
    ):
        assert name in kiyosi.__all__
        assert ([ast.unparse(base) for base in declarations[name].bases]) == (
            ["KiyosiError", "ValueError"]
            if name == "InvalidInputError"
            else ["InvalidInputError"]
            if name == "MissingHistoryError"
            else ["KiyosiError"]
        )
