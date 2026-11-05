"""Native Greek controls stay callable when engines are substituted."""

from typing import assert_type

from kiyosi import PricingResult
from kiyosi.instruments import EuropeanOption
from kiyosi.market import PricingContext
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    AnalyticHestonEuropeanEngine,
    CoxRossRubinsteinVanillaEngine,
    FiniteDifferenceHestonVanillaEngine,
    MonteCarloVanillaEngine,
    numerical_greeks,
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
            numerical_greeks(engine, option, context, "delta", spot_shift=0.001),
            PricingResult,
        )
