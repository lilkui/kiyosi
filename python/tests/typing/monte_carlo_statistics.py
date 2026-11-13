"""Every Monte Carlo product family exposes typed sampling estimates."""

from typing import assert_type

from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    ArithmeticAveragePriceOption,
    BarrierOption,
    CashOrNothingOption,
    DoubleBarrierOption,
    EuropeanOption,
    GeometricAveragePriceOption,
    PhoenixOption,
    SnowballOption,
)
from kiyosi.market import PricingContext
from kiyosi.pricing import (
    MonteCarloAccumulatorEngine,
    MonteCarloAsianEngine,
    MonteCarloAutocallableEngine,
    MonteCarloBarrierEngine,
    MonteCarloDigitalEngine,
    MonteCarloEstimate,
    MonteCarloUncertaintyStatus,
    MonteCarloVanillaEngine,
)


def sampling_estimates(
    cash: CashOrNothingOption,
    geometric: GeometricAveragePriceOption,
    arithmetic: ArithmeticAveragePriceOption,
    barrier: BarrierOption,
    double: DoubleBarrierOption,
    accumulator: Accumulator,
    snowball: SnowballOption,
    phoenix: PhoenixOption,
    context: PricingContext,
) -> None:
    assert_type(
        MonteCarloDigitalEngine().price_with_statistics(cash, context),
        MonteCarloEstimate,
    )
    asian = MonteCarloAsianEngine()
    assert_type(asian.price_with_seed(arithmetic, context), tuple[float, int | None])
    assert_type(
        asian.price_with_statistics(geometric, context).uncertainty_status,
        MonteCarloUncertaintyStatus,
    )
    assert_type(asian.price_with_statistics(geometric, context), MonteCarloEstimate)
    assert_type(asian.price_with_statistics(geometric, context).seed, int | None)
    assert_type(asian.price_with_statistics(arithmetic, context), MonteCarloEstimate)
    assert_type(
        MonteCarloBarrierEngine().price_with_statistics(barrier, context),
        MonteCarloEstimate,
    )
    assert_type(
        MonteCarloBarrierEngine().price_with_statistics(double, context),
        MonteCarloEstimate,
    )
    assert_type(
        MonteCarloAccumulatorEngine().price_with_statistics(accumulator, context),
        MonteCarloEstimate,
    )
    autocallable = MonteCarloAutocallableEngine()
    assert_type(
        autocallable.price_with_statistics(snowball, context), MonteCarloEstimate
    )
    assert_type(
        autocallable.price_with_statistics(phoenix, context), MonteCarloEstimate
    )
    assert_type(autocallable.with_resolved_seed(), MonteCarloAutocallableEngine)
    MonteCarloDigitalEngine().price_with_statistics(barrier, context)  # ty: ignore[invalid-argument-type]


def vanilla_statistics_contracts(
    european: EuropeanOption, american: AmericanOption, context: PricingContext
) -> None:
    vanilla = MonteCarloVanillaEngine()
    assert_type(vanilla.price(european, context), float)
    assert_type(vanilla.price(american, context), float)
    assert_type(vanilla.price_with_seed(american, context), tuple[float, int | None])
    assert_type(vanilla.price_with_statistics(european, context), MonteCarloEstimate)
    assert_type(vanilla.with_resolved_seed(), MonteCarloVanillaEngine)
    vanilla.price_with_statistics(american, context)  # ty: ignore[invalid-argument-type]
