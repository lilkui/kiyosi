"""Digital Monte Carlo terminal, likelihood and sampling contracts."""

import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import pytest
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.history import history_fields

START, END = (datetime(2025, 1, 1), datetime(2026, 1, 1))
GREEKS = (
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "vanna",
    "zomma",
    "rho",
)


def market(spot=100, volatility=0.2, valuation=START, rate=0.05, dividend=0.02):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
    )


def options():
    for direction in ("call", "put"):
        terms = {
            "option_type": direction,
            "strike": 100,
            "effective_time": START,
            "expiry_time": END,
        }
        yield (CashOrNothingOption(**terms, payout=10), False)
        yield (AssetOrNothingOption(**terms), True)


@pytest.mark.parametrize("remaining", (1, 365))
@pytest.mark.parametrize("spot", (95, 100, 105))
def test_prices_and_supported_greeks_match_analytic_contracts(
    subtests, remaining, spot
):
    time = remaining / 365
    width = 0.2 * math.sqrt(time)
    context = market(spot=spot, valuation=END - timedelta(days=remaining))
    for option, asset in options():
        expected = pricing.AnalyticDigitalEngine().price_with_greeks(
            option, context, ("delta", "gamma")
        )
        for sampling in ("pseudo_random", "sobol"):
            engine = pricing.MonteCarloDigitalEngine(
                path_count=65536, seed=42, sampling=sampling
            )
            with subtests.test(
                remaining=remaining,
                spot=spot,
                asset=asset,
                sampling=sampling,
            ):
                actual = engine.price_with_greeks(option, context, greeks="all")
                second = (
                    spot**2 * math.exp(-0.04 * time + width**2)
                    if asset
                    else 100 * math.exp(-0.1 * time)
                )
                errors = (
                    6 * math.sqrt(second / 65536),
                    6
                    * math.sqrt(second * (1 + 4 * width**2 if asset else 1) / 65536)
                    / spot
                    / width,
                    6
                    * math.sqrt(
                        second
                        * (2 + 9 * width**2 + 4 * width**4 if asset else 2 + width**2)
                        / 65536
                    )
                    / spot**2
                    / width**2,
                )
                for name, error in zip(
                    ("price", "delta", "gamma"), errors, strict=True
                ):
                    assert getattr(actual, name) == pytest.approx(
                        getattr(expected, name), rel=0, abs=error
                    )
                assert actual.price == engine.price(option, context)
                for name in GREEKS[2:]:
                    assert getattr(actual, name) is None


@pytest.mark.parametrize(
    "rate,dividend,volatility",
    ((-0.03, 0.01, 0.45), (0.01, 0.07, 0.3), (-0.02, -0.05, 0.15)),
)
def test_negative_rates_dividends_and_carry_match_analytic_values(
    rate, dividend, volatility
):
    context = market(rate=rate, dividend=dividend, volatility=volatility)
    for option, asset in options():
        expected = pricing.AnalyticDigitalEngine().price_with_greeks(
            option, context, ("delta", "gamma")
        )
        engine = pricing.MonteCarloDigitalEngine(
            path_count=65536, seed=42, sampling="sobol"
        )
        error = 0.001
        actual = engine.price_with_greeks(option, context, greeks="all")
        scale = 100 if asset else 10
        assert actual.price == pytest.approx(expected.price, rel=0, abs=error * scale)
        assert actual.delta == pytest.approx(
            expected.delta, rel=0, abs=error * scale / 100 / volatility
        )
        assert actual.gamma == pytest.approx(
            expected.gamma, rel=0, abs=error * scale / 10000 / volatility**2
        )


@pytest.mark.parametrize("sampling,paths", (("pseudo_random", 1025), ("sobol", 1024)))
@pytest.mark.parametrize("antithetic", (False, True))
def test_mc_threads_steps_selections_and_concurrency_share_samples(
    sampling, paths, antithetic
):
    settings = {
        "path_count": paths,
        "seed": 42,
        "sampling": sampling,
        "antithetic": antithetic,
    }
    for option, asset in options():
        engine = pricing.MonteCarloDigitalEngine(**settings)
        expected = engine.price_with_greeks(option, market(), greeks="all")
        for threads in (0, 1, 4):
            other = pricing.MonteCarloDigitalEngine(
                **settings, max_active_workers=threads
            )
            for name in ("delta", "gamma"):
                actual = other.price_with_greeks(option, market(), name)
                assert actual.price == expected.price
                assert getattr(actual, name) == getattr(expected, name)
                assert getattr(actual, "gamma" if name == "delta" else "delta") is None
        with ThreadPoolExecutor(max_workers=2) as pool:
            futures = [pool.submit(engine.price, option, market()) for _ in range(2)]
            results = [future.result() for future in futures]
        assert results == [expected.price, expected.price]


def test_requests_lifetimes_tiny_volatility_and_greek_overflow():
    for option, asset in options():
        for engine in (
            pricing.MonteCarloDigitalEngine(path_count=2, seed=42, antithetic=True),
        ):
            controlled = numerical_greeks(
                engine,
                option,
                market(),
                "rho",
                spot_shift=0.1,
                volatility_shift=0.001,
                rate_shift=0.001,
                time_shift=timedelta(days=2),
            )
            assert controlled.price == engine.price(option, market())
            assert controlled.status("rho") == "available"
            with pytest.raises(UnsupportedOperationError):
                engine.price_with_greeks(option, market(), "rho")
            with pytest.raises(TypeError):
                numerical_greeks(engine, option, market(), "delta", spot_shift=True)
            assert_rejection(
                InvalidInputError,
                engine.price,
                option,
                market(valuation=END + timedelta(days=1)),
            )
        for sigma in (1e-200, math.ulp(0.0)):
            context = market(
                volatility=sigma,
                rate=0,
                dividend=0,
                valuation=datetime(2026, 1, 1) - timedelta(microseconds=1),
            )
            engine = pricing.MonteCarloDigitalEngine(
                path_count=2, seed=42, antithetic=True
            )
            assert engine.price(option, context) == (50 if asset else 5)
            with pytest.raises(UnsupportedOperationError):
                engine.price_with_greeks(option, context, "rho")
            assert_rejection(
                ComputationFailureError,
                engine.price_with_greeks,
                option,
                context,
                "gamma",
            )


def test_defaults_representations_and_validation_order():
    mc = pricing.MonteCarloDigitalEngine()
    vanilla = pricing.MonteCarloVanillaEngine()
    for name in (
        "path_count",
        "seed",
        "max_active_workers",
        "sampling",
        "antithetic",
    ):
        assert getattr(mc, name) == getattr(vanilla, name)
        with pytest.raises(AttributeError):
            setattr(mc, name, getattr(mc, name))
    assert "MonteCarloDigitalEngine" in pricing.__all__
    for field in ("path_count", "max_active_workers", "seed"):
        for value in (True, 1.5, 2.0, "3", None if field != "seed" else object()):
            with pytest.raises(TypeError):
                pricing.MonteCarloDigitalEngine(**{field: value})
        with pytest.raises(OverflowError):
            pricing.MonteCarloDigitalEngine(**{field: 1 << 100})
    for field in ("sampling",):
        with pytest.raises(TypeError):
            pricing.MonteCarloDigitalEngine(**{field: True})
        with pytest.raises(ValueError):
            pricing.MonteCarloDigitalEngine(**{field: "unknown"})
    for fields in ({"antithetic": 1}, {"unknown": 1}):
        with pytest.raises(TypeError):
            pricing.MonteCarloDigitalEngine(**fields)
    for fields in (
        {"path_count": 0},
        {"path_count": 10000001},
        {"max_active_workers": -1},
        {"path_count": 3, "sampling": "sobol"},
    ):
        assert_rejection(
            InvalidInputError,
            pricing.MonteCarloDigitalEngine,
            **fields,
        )
    for seed in (-1, 2**64):
        with pytest.raises(OverflowError):
            pricing.MonteCarloDigitalEngine(seed=seed)
    for seed in (0, 2**64 - 1):
        assert pricing.MonteCarloDigitalEngine(seed=seed).seed == seed
    from kiyosi.instruments import EuropeanOption
    from kiyosi.market import RateTermStructure

    with pytest.raises(UnsupportedOperationError):
        mc.price(
            EuropeanOption(
                option_type="call",
                strike=100,
                effective_time=START,
                expiry_time=END,
            ),
            market(),
        )
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[0.95, (0.95) * 0.5],
    )
    node = market().replace(
        model_parameters=market().model_parameters.replace(risk_free_rate=curve),
        valuation_time=END,
    )
    for option, _ in options():
        assert_rejection(UnsupportedOperationError, mc.price, option, node)
        assert_rejection(
            UnsupportedOperationError,
            mc.price_with_greeks,
            option,
            node,
            [],
        )
        assert_rejection(
            UnsupportedOperationError,
            pricing.implied_volatility,
            mc,
            option,
            node,
            math.nan,
            lower_bound=0.0,
            upper_bound=4.0,
        )


def test_strict_expiry_and_solver_failures_leave_inputs_unchanged():
    for option, asset in options():
        for spot in (math.nextafter(100, 0), 100, math.nextafter(100, 101)):
            context = market(spot=spot, valuation=END)
            engine = pricing.MonteCarloDigitalEngine(path_count=1, seed=42)
            result = engine.price_with_greeks(option, context, greeks="all")
            pays = spot > 100 if option.option_type == "call" else spot < 100
            assert result.price == ((spot if asset else 10) if pays else 0)
            for name in GREEKS:
                assert getattr(result, name) is None
            assert_rejection(
                UnsupportedOperationError,
                pricing.implied_volatility,
                engine,
                option,
                context,
                result.price,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    for sampling in ("pseudo_random", "sobol"):
        for antithetic in (False, True):
            context = market()
            for option, asset in options():
                engine = pricing.MonteCarloDigitalEngine(
                    path_count=65536,
                    seed=42,
                    sampling=sampling,
                    antithetic=antithetic,
                )
                quote = engine.price(option, context)
                bounds = {
                    "lower_bound": 0.19999999 if asset else 0.1,
                    "upper_bound": 0.20000001 if asset else 0.22,
                    "parameter_tolerance": 1e-12,
                }
                sigma = pricing.implied_volatility(
                    engine,
                    option,
                    context,
                    quote,
                    **{"lower_bound": 0.0, "upper_bound": 4.0, **bounds},
                )
                # Restrict asset brackets to a price-continuous neighborhood of the quote.
                assert sigma == pytest.approx(
                    0.2, rel=0, abs=(0.002 if asset else 0.001)
                )
                repriced = context.replace(
                    model_parameters=context.model_parameters.replace(volatility=sigma)
                )
                assert engine.price(option, repriced) == pytest.approx(
                    quote, rel=0, abs=1e-8
                )
                assert_rejection(
                    RootNotBracketedError,
                    pricing.implied_volatility,
                    engine,
                    option,
                    context,
                    200,
                    lower_bound=0.1,
                    upper_bound=0.22,
                )
                for threads in (1, 4):
                    other = pricing.MonteCarloDigitalEngine(
                        path_count=65536,
                        seed=42,
                        sampling=sampling,
                        antithetic=antithetic,
                        max_active_workers=threads,
                    )
                    assert (
                        pricing.implied_volatility(
                            other,
                            option,
                            context,
                            quote,
                            **{"lower_bound": 0.0, "upper_bound": 4.0, **bounds},
                        )
                        == sigma
                    )
                with pytest.raises(TypeError):
                    pricing.implied_volatility(
                        engine,
                        option,
                        context,
                        True,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )
                with pytest.raises(TypeError):
                    pricing.implied_volatility(
                        engine,
                        option,
                        context,
                        quote,
                        max_iterations=1.5,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )
                assert engine.price(option, context) == quote
                assert context.model_parameters.volatility == 0.2
    unseeded = pricing.MonteCarloDigitalEngine(path_count=4096)
    option = next(options())[0]
    assert_rejection(
        InvalidInputError,
        pricing.implied_volatility,
        unseeded,
        option,
        market(),
        4.945810910532236,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    with pytest.raises((NonConvergenceError, RootNotBracketedError)):
        pricing.implied_volatility(
            unseeded.with_resolved_seed(),
            option,
            market(),
            4.945810910532236,
            lower_bound=0.1,
            upper_bound=0.3,
        )
    assert unseeded.seed is None


def test_scaled_payoffs_discounting_and_selected_overflow():
    engine = pricing.MonteCarloDigitalEngine(
        path_count=65536, sampling="sobol", seed=42
    )
    for amount, rate in ((1e-300, -710), (1e300, 710), (1e308, 0), (1e-310, 0)):
        for cls in (CashOrNothingOption, AssetOrNothingOption):
            spot = 100 if cls is CashOrNothingOption else min(amount, 1e300)
            option = cls(
                **history_fields(
                    option_type="call",
                    strike=spot,
                    effective_time=START,
                    expiry_time=END,
                    **({"payout": amount} if cls is CashOrNothingOption else {}),
                )
            )
            context = market(spot=spot, rate=rate, dividend=rate)
            expected = pricing.AnalyticDigitalEngine().price(option, context)
            assert engine.price(option, context) == pytest.approx(
                expected, rel=0, abs=0.002 * expected + 1e-320
            )
    option = CashOrNothingOption(
        option_type="call",
        strike=1e-200,
        payout=1e100,
        effective_time=START,
        expiry_time=END,
    )
    context = market(spot=1e-200, rate=0, dividend=0)
    delta = engine.price_with_greeks(option, context, "delta")
    assert math.isfinite(delta.delta)
    assert delta.gamma is None
    assert_rejection(
        ComputationFailureError,
        engine.price_with_greeks,
        option,
        context,
        "gamma",
    )
    assert engine.price(option, context) == delta.price
    overflowing = CashOrNothingOption(
        option_type="call",
        strike=100,
        payout=1e308,
        effective_time=START,
        expiry_time=END,
    )
    assert_rejection(
        ComputationFailureError,
        engine.price,
        overflowing,
        market(rate=-10, dividend=-10),
    )


def test_path_counts_statistics_and_live_quantlib_reference(subtests):
    # Reuse all 40 independent digital scenarios, selecting native delta/gamma only.
    import importlib

    import QuantLib as ql
    from test_binomial import contract

    digital = importlib.import_module("quantlib_reference.digital")
    oracle = importlib.import_module("quantlib_reference.oracle")
    oracle.require_version()
    config = oracle.load_fixture("digital.json")
    for identifier, inputs in digital.scenarios(config):
        cls = (
            CashOrNothingOption if inputs["payoff"] == "cash" else AssetOrNothingOption
        )
        option = cls(
            **history_fields(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_time=datetime.fromisoformat(inputs["effective_time"]),
                expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
                **({"payout": inputs["payout"]} if inputs["payoff"] == "cash" else {}),
            )
        )
        context = market(
            spot=inputs["spot"],
            volatility=inputs["volatility"],
            valuation=datetime.fromisoformat(inputs["valuation"]),
            rate=inputs["rate"],
            dividend=inputs["dividend"],
        )
        with ql.SavedSettings():
            reference, _ = oracle.reference(inputs, digital.STABILITY)
        actual = pricing.MonteCarloDigitalEngine(
            path_count=65536, seed=42, sampling="sobol"
        ).price_with_greeks(option, context, ["delta", "gamma"])
        time = (option.expiry_time - context.valuation_time).days / 365
        scale = inputs.get("payout", inputs["spot"])
        width = inputs["volatility"] * math.sqrt(time)
        for name, error in (
            ("price", 0.002 * scale),
            ("delta", 0.002 * scale / inputs["spot"] / width),
            ("gamma", 0.002 * scale / inputs["spot"] ** 2 / width**2),
        ):
            with subtests.test(case=identifier, measure=name):
                assert getattr(actual, name) == pytest.approx(
                    reference[name], rel=0, abs=error
                )
    for direction in ("call", "put"):
        option = contract(CashOrNothingOption, direction)
        expected = pricing.AnalyticDigitalEngine().price(option, market())
        discount = math.exp(-0.05)
        probability = expected / (10 * discount)
        variance = (10 * discount) ** 2 * probability * (1 - probability)
        for paths in (2048, 32768):
            errors = [
                pricing.MonteCarloDigitalEngine(
                    path_count=paths, seed=seed, max_active_workers=1
                ).price(option, market())
                - expected
                for seed in range(32)
            ]
            rmse = math.sqrt(sum(e * e for e in errors) / len(errors))
            assert (
                0.5 * math.sqrt(variance / paths)
                < rmse
                < 1.5 * math.sqrt(variance / paths)
            )
            assert abs(sum(errors) / len(errors)) < 4 * math.sqrt(
                variance / paths / len(errors)
            )
    for paths in (1, 2, 3, 511, 512, 513, 1025):
        option = next(options())[0]
        value = pricing.MonteCarloDigitalEngine(
            path_count=paths, antithetic=True, seed=42
        ).price(option, market(rate=0.02, dividend=0))
        paired = (paths // 2) * 10 * math.exp(-0.02) / paths
        unpaired = 10 * math.exp(-0.02) / paths if paths % 2 else 0
        assert abs(value - paired) < 1e-12 or abs(value - paired - unpaired) < 1e-12
