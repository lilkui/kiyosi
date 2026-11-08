"""Native Greek controls stay callable when engines are substituted."""

from datetime import timedelta
from typing import assert_type

from kiyosi import PricingResult
from kiyosi.instruments import (
    BarrierOption,
    CashOrNothingOption,
    DoubleBarrierOption,
    EuropeanOption,
)
from kiyosi.market import PricingContext
from kiyosi.pricing import (
    AnalyticDigitalEngine,
    AnalyticEuropeanEngine,
    AnalyticHestonEuropeanEngine,
    CoxRossRubinsteinVanillaEngine,
    FiniteDifferenceHestonVanillaEngine,
    FiniteDifferenceVanillaEngine,
    MonteCarloBarrierEngine,
    MonteCarloDoubleBarrierEngine,
    MonteCarloVanillaEngine,
    numerical_greeks,
)


def native_barrier_controls(
    single: BarrierOption, double: DoubleBarrierOption, context: PricingContext
) -> None:
    assert_type(
        MonteCarloBarrierEngine().price_with_greeks(
            single, context, "delta", spot_shift=0.001
        ),
        PricingResult,
    )
    assert_type(
        MonteCarloDoubleBarrierEngine().price_with_greeks(
            double, context, "delta", spot_shift=None
        ),
        PricingResult,
    )


def common_controls(option: EuropeanOption, context: PricingContext) -> None:
    for engine in (
        AnalyticEuropeanEngine(),
        AnalyticHestonEuropeanEngine(),
        CoxRossRubinsteinVanillaEngine(),
        FiniteDifferenceHestonVanillaEngine(),
        MonteCarloVanillaEngine(),
    ):
        assert_type(engine.price_with_greeks(option, context, "delta"), PricingResult)
    for engine in (
        AnalyticEuropeanEngine(),
        CoxRossRubinsteinVanillaEngine(),
        MonteCarloVanillaEngine(),
    ):
        assert_type(
            numerical_greeks(
                engine,
                option,
                context,
                "delta",
                spot_shift=0.001,
                time_shift=timedelta(seconds=1),
            ),
            PricingResult,
        )


def fd_configured_shifts(option: EuropeanOption, context: PricingContext) -> None:
    engine = FiniteDifferenceVanillaEngine(
        volatility_shift=0.00001, rate_shift=0.0002, time_shift=timedelta(seconds=60)
    )

    assert_type(engine.volatility_shift, float)
    assert_type(engine.rate_shift, float)
    assert_type(engine.time_shift, timedelta | None)
    assert_type(
        engine.price_with_greeks(option, context, ["delta", "vega"]), PricingResult
    )


def mixed_digital_shifts(option: CashOrNothingOption, context: PricingContext) -> None:
    assert_type(
        AnalyticDigitalEngine().price_with_greeks(
            option, context, ["delta", "gamma", "vega"], volatility_shift=0.00001
        ),
        PricingResult,
    )


def engine_replacement_types() -> None:
    engine = MonteCarloVanillaEngine(seed=42)
    assert_type(engine.replace(path_count=4096), MonteCarloVanillaEngine)
    assert_type(engine.replace(seed=None), MonteCarloVanillaEngine)
    assert_type(engine.replace(path_count=...), MonteCarloVanillaEngine)
    engine.replace(path_count=None)  # ty: ignore[invalid-argument-type]
    engine.replace(unknown=1)  # ty: ignore[unknown-argument]
