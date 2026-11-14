"""Public API checks, through the Rust extension."""

import math
import numbers
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime
from fractions import Fraction

import pytest
from kiyosi import ComputationFailureError, InvalidInputError
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
)
from kiyosi.pricing import AnalyticEuropeanEngine
from support.assertions import assert_decimal_equal


@pytest.fixture
def base_parameters():
    return BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
    )


def make_option(**changes):
    values = {
        "option_type": "call",
        "strike": 100.0,
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
    }
    values.update(changes)
    return EuropeanOption(**values)


@pytest.fixture
def make_context(base_parameters):
    def create(**changes):
        values = {
            "model_parameters": base_parameters,
            "spot_price": 100.0,
            "valuation_time": datetime(2025, 1, 1),
        }
        values.update(changes)
        return PricingContext(**values)

    return create


@pytest.mark.parametrize(
    "direction,expected", (("call", 9.227005508154036), ("put", 6.330080627549918))
)
def test_call_and_put_prices(direction, expected):
    parameters = BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
    )
    context = PricingContext(
        model_parameters=parameters,
        spot_price=100.0,
        valuation_time=datetime(2025, 1, 1),
    )
    option = EuropeanOption(
        option_type=direction,
        strike=100.0,
        effective_time=datetime(2025, 1, 1),
        expiry_time=datetime(2026, 1, 1),
    )
    assert AnalyticEuropeanEngine().price(option, context) == pytest.approx(
        expected, rel=0, abs=1e-12
    )


def test_valuation_and_expiry_boundaries(make_context, subtests):
    option, engine = make_option(), AnalyticEuropeanEngine()
    noon = make_context(valuation_time=datetime(2025, 1, 1, 12))
    assert engine.price(option, noon) < engine.price(option, make_context())
    expiry = make_context(spot_price=110, valuation_time=option.expiry_time)
    assert engine.price(option, expiry) == 10
    same_day = make_option(effective_time=option.expiry_time)
    assert engine.price(same_day, expiry) == 10
    for at in (
        datetime(2026, 1, 1, 0, 0, 0, 1),
        datetime(2026, 1, 2),
    ):
        context = make_context(valuation_time=at)
        with subtests.test(at=at), pytest.raises(InvalidInputError) as error:
            engine.price(option, context)
        assert type(error.value) is InvalidInputError


def test_timezone_free_datetimes_use_stored_components(make_context, subtests):
    class ShadowedTime(datetime):
        year = property(lambda self: 67562)
        hour = property(lambda self: 25)
        microsecond = property(lambda self: 1000001)
        tzinfo = property(lambda self: UTC)

    local = ShadowedTime(2025, 1, 1, 8, 9, 10, 123456)
    expected = datetime(2025, 1, 1, 8, 9, 10, 123456)
    assert make_context(valuation_time=local).valuation_time == expected
    assert make_option(effective_time=local).effective_time == expected
    for time in (
        datetime(1, 1, 1),
        datetime(1600, 1, 1),
        datetime(2500, 1, 1),
        datetime(9999, 12, 31),
        datetime.max,
    ):
        assert make_context(valuation_time=time).valuation_time == time
    for bad in (
        "2025-01-01",
        None,
        1,
        date(2025, 1, 1),
        datetime(2025, 1, 1, tzinfo=UTC),
    ):
        with subtests.test(bad=bad), pytest.raises(TypeError):
            make_context(valuation_time=bad)
        with subtests.test(bad=bad), pytest.raises(TypeError):
            make_option(effective_time=bad)


def test_strict_numeric_protocol_and_native_conversion_failures(make_context, subtests):
    for field in ("risk_free_rate", "dividend_yield", "volatility"):
        for bad in (True, False, "0.2", None, complex(1)):
            values = {
                "risk_free_rate": 0.05,
                "dividend_yield": 0.02,
                "volatility": 0.2,
            }
            values[field] = bad
            with subtests.test(field=field, bad=bad), pytest.raises(TypeError):
                BlackScholesMertonParameters(**values)
    for bad in (True, "100", None):
        with pytest.raises(TypeError):
            make_option(strike=bad)
        with pytest.raises(TypeError):
            make_context(spot_price=bad)
    with pytest.raises(OverflowError):
        make_option(strike=10**1000)
    assert make_option(strike=Fraction(201, 2)).strike == 100.5

    @numbers.Real.register
    class BrokenReal:
        def __float__(self):
            raise MemoryError("conversion failed")

    with pytest.raises(MemoryError):
        make_option(strike=BrokenReal())
    with pytest.raises(TypeError):
        make_option(option_type=1)
    with pytest.raises(ValueError):
        make_option(option_type="Call")
    with pytest.raises(TypeError):
        EuropeanOption("call", 100, datetime(2025, 1, 1), datetime(2026, 1, 1))


def test_domain_errors_and_failed_construction_preserve_inputs(make_context, subtests):
    context, option = make_context(), make_option()
    for bad in (0, -1, math.nan, math.inf, -math.inf):
        factories = (
            (
                lambda bad=bad: make_option(strike=bad),
                InvalidInputError,
            ),
            (
                lambda bad=bad: make_context(spot_price=bad),
                InvalidInputError,
            ),
            (
                lambda bad=bad: BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=bad
                ),
                InvalidInputError,
            ),
        )
        for make, expected_error in factories if bad != 0 else factories[:2]:
            with (
                subtests.test(bad=bad, expected_error=expected_error),
                pytest.raises(expected_error) as error,
            ):
                make()
            assert type(error.value) is expected_error
    for field, expected_error in (
        ("risk_free_rate", InvalidInputError),
        ("dividend_yield", InvalidInputError),
    ):
        values = {"risk_free_rate": 0, "dividend_yield": 0, "volatility": 0.2}
        values[field] = math.inf
        with pytest.raises(expected_error) as error:
            BlackScholesMertonParameters(**values)
        assert type(error.value) is expected_error
    with pytest.raises(InvalidInputError) as error:
        make_option(effective_time=datetime(2027, 1, 1))
    assert type(error.value) is InvalidInputError
    assert_decimal_equal(
        AnalyticEuropeanEngine().price(option, context), 9.227005508154036
    )


def test_owned_readonly_values_and_concurrent_pricing(make_context):
    context, option, engine = (
        make_context(),
        make_option(),
        AnalyticEuropeanEngine(),
    )
    saved = context.model_parameters
    with pytest.raises(AttributeError):
        context.spot_price = 200
    with pytest.raises(AttributeError):
        saved.volatility = 0
    with pytest.raises(AttributeError):
        option.strike = 0
    expected = engine.price(option, context)
    with ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(engine.price, option, context) for _ in range(16)]
        prices = [future.result() for future in futures]
        distinct = list(
            pool.map(
                lambda _: AnalyticEuropeanEngine().price(make_option(), make_context()),
                range(16),
            )
        )
    assert prices == [expected] * 16
    assert distinct == prices
    del context
    assert saved.volatility == 0.2


def test_extreme_numerical_regressions(make_context, subtests):
    # Independent high-precision references for tail prices and extreme scales.
    for direction, spot, strike, sigma, expected in (
        ("call", 100, 100.00000000013, 1e-13, 4.6459080591698985e-51),
        ("put", 100, 99.99999999987, 1e-13, 4.645908058131258e-51),
        ("call", 100, 100.00099900499002, 1e-6, 8.276374696508508e-29),
        ("call", 100, 100.00100100501001, 1e-6, 6.749830430787083e-29),
        ("put", 100, 99.99700004499955, 1e-6, 1.6319322561070434e-203),
        ("call", 1e300, 1e304, 0.2, 1.1367038364232515e-163),
        ("put", 1e304, 1e300, 0.2, 1.1367038364232515e-163),
        ("call", 100, 100, 1e-200, 3.989422804014327e-199),
    ):
        model = BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=sigma
        )
        with subtests.test(direction=direction, strike=strike, sigma=sigma):
            actual = AnalyticEuropeanEngine().price(
                make_option(option_type=direction, strike=strike),
                make_context(model_parameters=model, spot_price=spot),
            )
            assert math.isclose(actual, expected, rel_tol=1e-8)
    model = BlackScholesMertonParameters(
        risk_free_rate=-1000, dividend_yield=0, volatility=0.3
    )
    with pytest.raises(ComputationFailureError) as error:
        AnalyticEuropeanEngine().price(
            make_option(option_type="put"), make_context(model_parameters=model)
        )
    assert type(error.value) is ComputationFailureError
