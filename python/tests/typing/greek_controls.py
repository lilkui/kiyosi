"""Native Greek controls stay callable when engines are substituted."""

from datetime import timedelta
from typing import assert_type

from kiyosi import PricingResult
from kiyosi.instruments import BarrierOption, DoubleBarrierOption, EuropeanOption
from kiyosi.market import PricingContext
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    AnalyticHestonEuropeanEngine,
    CoxRossRubinsteinVanillaEngine,
    FiniteDifferenceHestonVanillaEngine,
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
