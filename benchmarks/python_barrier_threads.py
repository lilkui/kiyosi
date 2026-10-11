"""Compare fixed-work barrier pricing with one and four Python workers."""

import statistics
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import date, timedelta

from kiyosi import pricing
from kiyosi.instruments import BarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


def main():
    start, end = date(2015, 1, 1), date(2025, 1, 1)
    dates = [
        start + timedelta(days=day)
        for day in range(1, (end - start).days + 1)
        if (start + timedelta(days=day)).weekday() < 5
    ]
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = pricing.AnalyticBarrierEngine()

    def run(option, count, expected):
        for _ in range(count):
            assert engine.price(option, context) == expected

    for mode, count in (("continuous", 100_000), ("scheduled", 20_000)):
        option = BarrierOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=end,
            barrier_level=80,
            barrier_type="down_and_out",
            observation_mode=mode,
            observation_dates=dates if mode == "scheduled" else [],
        )
        expected = engine.price(option, context)
        for workers in (1, 4):
            samples = []
            with ThreadPoolExecutor(max_workers=workers) as pool:
                for _ in range(5):
                    started = time.perf_counter()
                    futures = [
                        pool.submit(run, option, count // workers, expected)
                        for _ in range(workers)
                    ]
                    for future in futures:
                        future.result()
                    samples.append((time.perf_counter() - started) * 1000)
            print(
                f"{mode}: workers={workers}, calls={count}, "
                f"median={statistics.median(samples):.2f} ms, price={expected:.17g}",
                flush=True,
            )


if __name__ == "__main__":
    main()
