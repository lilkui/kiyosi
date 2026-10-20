import unittest
from datetime import date, timedelta

from kiyosi import pricing
from kiyosi.instruments import (
    Accumulator,
    BarrierOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)

start, end = date(2024, 1, 1), date(2025, 1, 1)
observations = [start + timedelta(days=i) for i in range(1, 367)]
cases = [
    (
        pricing.FiniteDifferenceAccumulatorEngine,
        Accumulator(
            strike=100,
            knock_out_level=115,
            daily_quantity=1,
            acceleration_factor=2,
            effective_date=start,
            expiry_date=end,
        ),
    ),
    (
        pricing.FiniteDifferenceBarrierEngine,
        BarrierOption(
            option_type="call",
            strike=100,
            barrier_level=110,
            barrier_type="up_and_out",
            observation_mode="scheduled",
            observation_dates=observations,
            rebate=2,
            effective_date=start,
            expiry_date=end,
        ),
    ),
]


class CalendarIntervalTests(unittest.TestCase):
    def test_weekday_and_weekend_intervals_retain_reference_values(self):
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100.25,
            valuation_time=start,
            calendar=weekdays_calendar(),
        )
        expected = [
            (-1492.5835821073674, 225.91508609768806, -18.07108836828621),
            (-1489.5563876800627, 226.06642053750477, -18.104752640872828),
        ]
        for index, (cls, option) in enumerate(cases[:1]):
            for offset, scheme in enumerate(("crank_nicolson", "implicit_euler")):
                with self.subTest(engine=cls.__name__, scheme=scheme):
                    engine = cls(
                        asset_step_count=80,
                        time_step_count=1,
                        asset_upper_boundary=400,
                        scheme=scheme,
                    )
                    result = engine.price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    self.assertEqual(engine.price(option, context), result.price)
                    for name, value in zip(
                        ("price", "delta", "gamma"),
                        expected[2 * index + offset],
                        strict=True,
                    ):
                        self.assertAlmostEqual(getattr(result, name), value, delta=1e-9)

    def test_daily_prices_and_greeks_retain_reference_values(self):
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100.25,
            valuation_time=start,
            calendar=all_days_calendar(),
        )
        expected = [
            (-2097.8369545877604, 314.8258247821256, -25.16084216695802),
            (-2095.426480852846, 314.9393707855528, -25.18852830232388),
            (-2105.2103901681744, 314.4883320755036, -25.075183399852207),
            (1.3493499685691974, 0.05873892375941932, 0.00011930380980306406),
            (1.3468904122711003, 0.058637636418814774, 0.00012654044204111787),
            (1.3518636988741737, 0.0588457671037437, 0.00011178373357470883),
        ]
        for index, (cls, option) in enumerate(cases):
            for offset, scheme in enumerate(
                ("crank_nicolson", "implicit_euler", "explicit_euler")
            ):
                with self.subTest(engine=cls.__name__, scheme=scheme):
                    engine = cls(
                        asset_step_count=80,
                        time_step_count=1,
                        asset_upper_boundary=400,
                        scheme=scheme,
                    )
                    result = engine.price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    self.assertEqual(engine.price(option, context), result.price)
                    for name, value in zip(
                        ("price", "delta", "gamma"),
                        expected[3 * index + offset],
                        strict=True,
                    ):
                        self.assertAlmostEqual(getattr(result, name), value, delta=1e-9)
