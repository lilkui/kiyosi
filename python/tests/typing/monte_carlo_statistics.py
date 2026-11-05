"""Every Monte Carlo product family exposes typed sampling estimates."""

from typing import assert_type

from kiyosi.instruments import (
    ArithmeticAveragePriceOption,
    BarrierOption,
    CashOrNothingOption,
    DoubleBarrierOption,
    GeometricAveragePriceOption,
)
from kiyosi.market import PricingContext
from kiyosi.pricing import (
    MonteCarloAsianEngine,
    MonteCarloBarrierEngine,
    MonteCarloDigitalEngine,
    MonteCarloDoubleBarrierEngine,
    MonteCarloEstimate,
    MonteCarloUncertaintyStatus,
)


def sampling_estimates(
    cash: CashOrNothingOption,
    geometric: GeometricAveragePriceOption,
    arithmetic: ArithmeticAveragePriceOption,
    barrier: BarrierOption,
    double: DoubleBarrierOption,
    context: PricingContext,
) -> None:
    assert_type(
        MonteCarloDigitalEngine().price_with_statistics(cash, context),
        MonteCarloEstimate,
    )
    asian = MonteCarloAsianEngine()
    assert_type(
        asian.price_with_statistics(geometric, context).uncertainty_status,
        MonteCarloUncertaintyStatus,
    )
    assert_type(asian.price_with_statistics(geometric, context), MonteCarloEstimate)
    assert_type(asian.price_with_statistics(arithmetic, context), MonteCarloEstimate)
    assert_type(
        MonteCarloBarrierEngine().price_with_statistics(barrier, context),
        MonteCarloEstimate,
    )
    assert_type(
        MonteCarloDoubleBarrierEngine().price_with_statistics(double, context),
        MonteCarloEstimate,
    )
    MonteCarloDigitalEngine().price_with_statistics(barrier, context)  # ty: ignore[invalid-argument-type]
