"""Compare fixed-work Asian Greek requests with one and four Python workers."""

from datetime import date

from kiyosi import pricing
from kiyosi.instruments import ArithmeticAveragePriceOption, GeometricAveragePriceOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from python_threads import median_thread_time


def main():
    start, end = date(2025, 1, 1), date(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    for kind, engine in (
        (GeometricAveragePriceOption, pricing.AnalyticGeometricAveragePriceEngine()),
        (
            ArithmeticAveragePriceOption,
            pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
        ),
    ):
        option = kind(
            option_type="call",
            strike=100,
            effective_date=start,
            averaging_start_date=start,
            expiry_date=end,
        )
        for selection in ({"greeks": "delta"}, {"all_greeks": True}):
            expected = engine.price_with_greeks(option, context, **selection)

            def run(
                count,
                engine=engine,
                option=option,
                selection=selection,
                expected=expected,
            ):
                for _ in range(count):
                    result = engine.price_with_greeks(option, context, **selection)
                assert result.price == expected.price and result.delta == expected.delta
                assert result.gamma == expected.gamma and result.vega == expected.vega

            for workers in (1, 4):
                elapsed = median_thread_time(run, 80_000, workers)
                print(
                    f"{kind.__name__}: {selection}, workers={workers}, calls=80000, "
                    f"median={elapsed:.2f} ms",
                    flush=True,
                )


if __name__ == "__main__":
    main()
