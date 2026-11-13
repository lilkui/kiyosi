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
    BjerksundStenslandVanillaEngine,
    CoxRossRubinsteinVanillaEngine,
    FiniteDifferenceHestonVanillaEngine,
    FiniteDifferenceVanillaEngine,
    MonteCarloBarrierEngine,
    MonteCarloDoubleBarrierEngine,
    MonteCarloVanillaEngine,
    QuadratureVanillaEngine,
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
        assert_type(
            engine.price_with_greeks(
                option,
                context,
                "delta",
                spot_shift=0.1,
                volatility_shift=None,
                rate_shift=0.001,
                time_shift=timedelta(seconds=60),
            ),
            PricingResult,
        )
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
                volatility_shift=None,
                time_shift=timedelta(seconds=1),
            ),
            PricingResult,
        )


def fd_configured_shifts(option: EuropeanOption, context: PricingContext) -> None:
    engine = FiniteDifferenceVanillaEngine(
        volatility_shift=0.00001, rate_shift=0.0002, time_shift=timedelta(seconds=60)
    )

    assert_type(engine.volatility_shift, float | None)
    assert_type(engine.rate_shift, float)
    assert_type(engine.time_shift, timedelta | None)
    assert_type(
        engine.price_with_greeks(option, context, ["delta", "vega"]), PricingResult
    )
    assert_type(
        numerical_greeks(
            engine,
            option,
            context,
            "rho",
            spot_shift=Ellipsis,
            volatility_shift=Ellipsis,
            rate_shift=Ellipsis,
            time_shift=Ellipsis,
        ),
        PricingResult,
    )


def mixed_digital_shifts(option: CashOrNothingOption, context: PricingContext) -> None:
    assert_type(
        AnalyticDigitalEngine().price_with_greeks(
            option, context, ["delta", "gamma", "vega"], volatility_shift=0.00001
        ),
        PricingResult,
    )


def engine_replacement_types() -> None:
    AnalyticEuropeanEngine().replace()  # ty: ignore[unresolved-attribute]
    engine = MonteCarloVanillaEngine(seed=42)
    assert_type(engine.replace(path_count=4096), MonteCarloVanillaEngine)
    assert_type(engine.replace(seed=None), MonteCarloVanillaEngine)
    assert_type(engine.replace(path_count=...), MonteCarloVanillaEngine)
    engine.replace(path_count=None)  # ty: ignore[invalid-argument-type]
    engine.replace(unknown=1)  # ty: ignore[unknown-argument]
    finite_difference = FiniteDifferenceVanillaEngine()
    assert_type(
        finite_difference.replace(asset_step_count=400), FiniteDifferenceVanillaEngine
    )
    assert_type(
        finite_difference.replace(spot_shift=None), FiniteDifferenceVanillaEngine
    )
    finite_difference.replace(asset_step_count=None)  # ty: ignore[invalid-argument-type]
    finite_difference.replace(unknown=1)  # ty: ignore[unknown-argument]


def inherited_engine_instrument_types(
    vanilla: EuropeanOption, digital: CashOrNothingOption, context: PricingContext
) -> None:
    for engine in (QuadratureVanillaEngine(), FiniteDifferenceVanillaEngine()):
        assert_type(engine.price(vanilla, context), float)
        assert_type(engine.price_with_greeks(vanilla, context, "delta"), PricingResult)
        engine.price(digital, context)  # ty: ignore[invalid-argument-type]
        engine.price_with_greeks(digital, context, "delta")  # ty: ignore[invalid-argument-type]
        engine.greek_method(digital, context)  # ty: ignore[invalid-argument-type]
        engine.active_greek_controls(digital, context)  # ty: ignore[invalid-argument-type]
    BjerksundStenslandVanillaEngine().price(vanilla, context)  # ty: ignore[invalid-argument-type]
