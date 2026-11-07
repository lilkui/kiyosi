"""Numeric protocols accepted by the public Python interface."""

from datetime import datetime
from datetime import time as clock_time
from decimal import Decimal
from fractions import Fraction
from typing import assert_type

from kiyosi.instruments import EuropeanOption, PhoenixOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    fixed_interval_schedule,
)
from kiyosi.pricing import (
    CoxRossRubinsteinVanillaEngine,
    MonteCarloVanillaEngine,
    PhoenixCouponQuote,
    QuadratureAutocallableEngine,
    implied_phoenix_coupon,
)


class Index:
    def __index__(self) -> int:
        return 42


def phoenix_quote(note: PhoenixOption, context: PricingContext) -> None:
    quote = implied_phoenix_coupon(QuadratureAutocallableEngine(), note, context, 1)
    assert_type(quote, PhoenixCouponQuote)
    assert_type(note.replace(**quote), PhoenixOption)
    note.replace(coupon_rate=quote)  # ty: ignore[no-matching-overload]


def numeric_protocol_inputs() -> None:
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    assert_type(CoxRossRubinsteinVanillaEngine(step_count=Index()).step_count, int)
    assert_type(MonteCarloVanillaEngine(seed=Index()).seed, int | None)
    schedule = fixed_interval_schedule(
        observation_clock_time=clock_time(),
        start_time=start,
        end_time=end,
        interval_days=Index(),
    )
    assert_type(schedule[Index()], datetime)
    option = EuropeanOption(
        option_type="call",
        strike=Fraction(201, 2),
        effective_time=start,
        expiry_time=end,
    )
    assert_type(option.strike, float)
    assert_type(option.replace(strike=Fraction(203, 2)), EuropeanOption)
    assert_type(option.replace(strike=Decimal("100.5")), EuropeanOption)
    model = BlackScholesMertonParameters(
        risk_free_rate=Fraction(1, 32), dividend_yield=0, volatility=Decimal("0.25")
    )
    assert_type(model.volatility, float)
    assert_type(model.flat_risk_free_rate, float)
    assert_type(model.flat_dividend_yield, float)
    assert_type(model.risk_free_curve.base_factors, list[float])
    assert_type(model.risk_free_curve.parallel_shift, float)
    assert_type(
        model.replace(risk_free_rate=model.risk_free_curve),
        BlackScholesMertonParameters,
    )
    CoxRossRubinsteinVanillaEngine(step_count="42")  # ty: ignore[invalid-argument-type]
    option.replace(strike="100")  # ty: ignore[invalid-argument-type]
