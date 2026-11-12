"""Exact deterministic BSM prices through supported public pricing engines."""

import math
from datetime import datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import InvalidInputError, pricing
from kiyosi import instruments as ins
from kiyosi.instruments import KnockInObservation
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)

START, END = datetime(2025, 1, 6), datetime(2025, 1, 10)


def cases():
    life = {"effective_time": START, "expiry_time": END}
    vanilla = dict(life, option_type="call", strike=90)
    lambda prefix, families: tuple(prefix + family + "Engine" for family in families)
    yield (
        ins.EuropeanOption(**vanilla),
        (
            "AnalyticEuropeanEngine",
            "CoxRossRubinsteinVanillaEngine",
            "QuadratureVanillaEngine",
            "FiniteDifferenceVanillaEngine",
            "MonteCarloVanillaEngine",
        ),
        10,
    )
    yield (
        ins.AmericanOption(**vanilla),
        (
            "BjerksundStenslandVanillaEngine",
            "CoxRossRubinsteinVanillaEngine",
            "QuadratureVanillaEngine",
            "FiniteDifferenceVanillaEngine",
            "MonteCarloVanillaEngine",
        ),
        10,
    )
    for cls, expected, fields in (
        (ins.CashOrNothingOption, 10, {"payout": 10}),
        (ins.AssetOrNothingOption, 100, {}),
    ):
        yield (
            cls(**vanilla, **fields),
            (
                "AnalyticDigitalEngine",
                "CoxRossRubinsteinDigitalEngine",
                "QuadratureDigitalEngine",
                "FiniteDifferenceDigitalEngine",
                "MonteCarloDigitalEngine",
            ),
            expected,
        )
    for cls, analytic in (
        (ins.GeometricAveragePriceOption, "AnalyticGeometricAveragePriceEngine"),
        (
            ins.ArithmeticAveragePriceOption,
            "TurnbullWakemanArithmeticAveragePriceEngine",
        ),
    ):
        yield (
            cls(**vanilla, averaging_start_time=START),
            (analytic, "MonteCarloAsianEngine"),
            10,
        )
    yield (
        ins.BarrierOption(**vanilla, barrier_level=80, barrier_type="down_and_out"),
        (
            "AnalyticBarrierEngine",
            "CoxRossRubinsteinBarrierEngine",
            "QuadratureBarrierEngine",
            "FiniteDifferenceBarrierEngine",
            "MonteCarloBarrierEngine",
        ),
        10,
    )
    yield (
        ins.DoubleBarrierOption(
            **vanilla, lower_barrier=80, upper_barrier=120, barrier_type="knock_out"
        ),
        (
            "AnalyticDoubleBarrierEngine",
            "FiniteDifferenceDoubleBarrierEngine",
            "MonteCarloDoubleBarrierEngine",
        ),
        10,
    )
    for payout, expected in ((None, 100), (10, 10)):
        yield (
            ins.BinaryBarrierOption(
                payoff_type="cash" if payout is not None else "asset",
                **vanilla,
                barrier_level=80,
                barrier_type="down_and_out",
                payout=payout,
            ),
            ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
            expected,
        )
        yield (
            ins.TouchOption(
                payoff_type="cash" if payout is not None else "asset",
                **life,
                barrier_level=80,
                barrier_type="down_and_out",
                payout=payout,
            ),
            ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
            expected,
        )
    yield (
        ins.DoubleTouchOption(
            **life,
            lower_barrier=80,
            upper_barrier=120,
            barrier_type="knock_out",
            payout=10,
        ),
        ("AnalyticDoubleTouchEngine", "FiniteDifferenceDoubleTouchEngine"),
        10,
    )
    obs = ins.autocallable_observations([END], [120], knock_out_coupon_rates=[0])
    note = dict(life, observations=obs)
    downside = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 0,
    }
    for contract in (
        ins.SnowballOption(
            calendar=weekdays_calendar(),
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            **note,
            **downside,
            maturity_coupon_rate=0.1,
        ),
        ins.PhoenixOption(
            calendar=weekdays_calendar(),
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            **dict(
                note,
                observations=ins.autocallable_observations(
                    [END], [120], coupon_barrier_levels=[0]
                ),
            ),
            **downside,
            coupon_per_period=0.1,
        ),
        ins.BinarySnowballOption(
            calendar=weekdays_calendar(),
            **note,
            maturity_coupon_rate=0.1,
        ),
        ins.TernarySnowballOption(
            calendar=weekdays_calendar(),
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            **note,
            knock_in_level=80,
            maturity_coupon_rate=0.1,
            minimum_coupon_rate=0,
        ),
    ):
        yield (
            contract,
            (
                "QuadratureAutocallableEngine",
                "FiniteDifferenceAutocallableEngine",
                "MonteCarloAutocallableEngine",
            ),
            1.1 if isinstance(contract, ins.PhoenixOption) else 1 + 0.1 * 4 / 365,
        )
    yield (
        ins.Accumulator(
            observation_clock_time=clock_time(),
            **life,
            strike=90,
            knock_out_level=120,
            daily_quantity=1,
            acceleration_factor=2,
        ),
        (
            "QuadratureAccumulatorEngine",
            "FiniteDifferenceAccumulatorEngine",
            "MonteCarloAccumulatorEngine",
        ),
        50,
    )
    yield (
        ins.RangeAccrualOption(
            observation_clock_time=clock_time(),
            **life,
            notional=1000,
            annual_coupon_rate=0.1,
            lower_bound=100,
            upper_bound=110,
            calendar=all_days_calendar(),
        ),
        ("AnalyticRangeAccrualEngine",),
        100 * 4 / 365,
    )


def scenarios():
    for option, engines, expected in cases():
        yield option, engines, expected
        if isinstance(
            option,
            (
                ins.EuropeanOption,
                ins.AmericanOption,
                ins.GeometricAveragePriceOption,
                ins.ArithmeticAveragePriceOption,
                ins.CashOrNothingOption,
                ins.AssetOrNothingOption,
                ins.BarrierOption,
                ins.DoubleBarrierOption,
                ins.BinaryBarrierOption,
            ),
        ):
            yield option.replace(option_type="put", strike=110), engines, expected


def test_zero_volatility_prices_and_statistics_agree_across_supported_engines(subtests):
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0
        ),
        spot_price=100,
        valuation_time=START,
    )
    for option, engines, expected in scenarios():
        for name in engines:
            with subtests.test(contract=type(option).__name__, engine=name):
                engine = getattr(pricing, name)(
                    **(
                        {"path_count": 16, "seed": 42, "max_active_workers": 1}
                        if name.startswith("MonteCarlo")
                        else {}
                    )
                )
                assert engine.price(option, context) == pytest.approx(
                    expected, rel=0, abs=1e-10
                )
                result = engine.price_with_greeks(option, context, "all")
                assert result.price == pytest.approx(expected, rel=0, abs=1e-10)
                if isinstance(option, ins.RangeAccrualOption):
                    assert result.status("delta") == "undefined"
                else:
                    expected_delta = (
                        1
                        if isinstance(option, ins.AssetOrNothingOption)
                        else 5
                        if isinstance(option, ins.Accumulator)
                        else (1 if option.payout is None else 0)
                        if isinstance(
                            option,
                            (
                                ins.BinaryBarrierOption,
                                ins.TouchOption,
                                ins.CashOrNothingOption,
                            ),
                        )
                        else 0
                        if isinstance(
                            option,
                            (
                                ins.DoubleTouchOption,
                                ins.SnowballOption,
                                ins.PhoenixOption,
                                ins.BinarySnowballOption,
                                ins.TernarySnowballOption,
                            ),
                        )
                        else -1
                        if option.option_type == "put"
                        else 1
                    )
                    assert result.delta is not None
                    assert result.delta == pytest.approx(
                        expected_delta, rel=0, abs=1e-9
                    )
                for greek in result.requested:
                    value = getattr(result, greek)
                    if value is not None:
                        assert math.isfinite(value)
                if name.startswith("MonteCarlo") and not isinstance(
                    option, ins.AmericanOption
                ):
                    estimate = engine.price_with_statistics(option, context)
                    assert estimate.price == pytest.approx(expected, rel=0, abs=1e-10)
                    assert estimate.standard_error == 0


def test_continuous_and_scheduled_hits_use_exact_deterministic_payment_times(subtests):
    end = START + timedelta(days=365)
    life = {"effective_time": START, "expiry_time": end}
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.1, dividend_yield=0.02, volatility=0
        ),
        spot_price=100,
        valuation_time=START,
    )
    hit = math.log(1.05) / 0.08
    vanilla = 100 * math.exp(-0.02) - 90 * math.exp(-0.1)
    checks = [
        (
            ins.BarrierOption(
                **life,
                option_type="call",
                strike=90,
                barrier_level=105,
                barrier_type="up_and_out",
                rebate=7,
                rebate_timing="at_hit",
            ),
            (
                "AnalyticBarrierEngine",
                "CoxRossRubinsteinBarrierEngine",
                "QuadratureBarrierEngine",
                "FiniteDifferenceBarrierEngine",
                "MonteCarloBarrierEngine",
            ),
            7 * math.exp(-0.1 * hit),
        ),
        (
            ins.BarrierOption(
                **life,
                option_type="call",
                strike=90,
                barrier_level=105,
                barrier_type="up_and_in",
            ),
            (
                "AnalyticBarrierEngine",
                "FiniteDifferenceBarrierEngine",
                "MonteCarloBarrierEngine",
            ),
            vanilla,
        ),
        (
            ins.TouchOption(
                payoff_type="cash",
                **life,
                barrier_level=105,
                barrier_type="up_and_in",
                payout=7,
                settlement_timing="at_hit",
            ),
            ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
            7 * math.exp(-0.1 * hit),
        ),
        (
            ins.TouchOption(
                payoff_type="asset",
                **life,
                barrier_level=105,
                barrier_type="up_and_in",
                payout=None,
                settlement_timing="at_hit",
            ),
            ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
            105 * math.exp(-0.1 * hit),
        ),
        (
            ins.DoubleTouchOption(
                **life,
                lower_barrier=80,
                upper_barrier=105,
                barrier_type="knock_in",
                payout=7,
                settlement_timing="at_hit",
            ),
            ("AnalyticDoubleTouchEngine", "FiniteDifferenceDoubleTouchEngine"),
            7 * math.exp(-0.1 * hit),
        ),
        (
            ins.TouchOption(
                payoff_type="cash",
                **life,
                barrier_level=105,
                barrier_type="up_and_in",
                payout=7,
                settlement_timing="at_hit",
                observation_times=[end],
            ),
            ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
            7 * math.exp(-0.1),
        ),
    ]
    for option, engines, expected in checks:
        for name in engines:
            with subtests.test(contract=type(option).__name__, engine=name):
                assert getattr(pricing, name)().price(option, context) == pytest.approx(
                    expected, rel=0, abs=1e-10
                )


def test_piecewise_rate_paths_detect_hits_before_reversing():
    from kiyosi.market import RateTermStructure

    middle, end = START + timedelta(days=180), START + timedelta(days=365)
    curve = RateTermStructure(
        reference_time=START,
        times=[middle, end],
        factors=[math.exp(-0.2 * 180 / 365), math.exp(0.2 * 5 / 365)],
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=curve, dividend_yield=0, volatility=0
        ),
        spot_price=100,
        valuation_time=START,
    )
    continuous = ins.TouchOption(
        payoff_type="cash",
        effective_time=START,
        expiry_time=end,
        barrier_level=105,
        barrier_type="up_and_in",
        payout=7,
        settlement_timing="at_hit",
    )
    engine = pricing.FiniteDifferenceBinaryBarrierEngine()
    assert engine.price(continuous, context) == pytest.approx(
        7 / 1.05, rel=0, abs=1e-10
    )
    scheduled = continuous.replace(
        observation_times=[end],
        touch_state=continuous.touch_state,
    )
    assert engine.price(scheduled, context) == 0


def test_american_exercise_and_seasoned_asian_averages_are_exact(subtests):
    end = START + timedelta(days=365)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.1, dividend_yield=0.09, volatility=0
        ),
        spot_price=100,
        valuation_time=START,
    )
    american = ins.AmericanOption(
        option_type="call", strike=90.5, effective_time=START, expiry_time=end
    )
    exercise = math.log(0.1 * 90.5 / (0.09 * 100)) / 0.01
    expected = 100 * math.exp(-0.09 * exercise) - 90.5 * math.exp(-0.1 * exercise)
    for name in (
        "BjerksundStenslandVanillaEngine",
        "CoxRossRubinsteinVanillaEngine",
        "QuadratureVanillaEngine",
        "FiniteDifferenceVanillaEngine",
        "MonteCarloVanillaEngine",
    ):
        with subtests.test(engine=name):
            assert getattr(pricing, name)().price(american, context) == pytest.approx(
                expected, rel=0, abs=1e-10
            )
    context = context.replace(
        valuation_time=START + timedelta(days=180),
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.08, dividend_yield=0.02, volatility=0
        ),
    )
    remaining = 185 / 365
    arithmetic = 80 * 180 / 365 + 100 * math.expm1(0.06 * remaining) / 0.06
    geometric = math.exp(
        math.log(80) * 180 / 365 + remaining * (math.log(100) + 0.06 * remaining / 2)
    )
    for cls, analytic, average in (
        (
            ins.ArithmeticAveragePriceOption,
            "TurnbullWakemanArithmeticAveragePriceEngine",
            arithmetic,
        ),
        (
            ins.GeometricAveragePriceOption,
            "AnalyticGeometricAveragePriceEngine",
            geometric,
        ),
    ):
        option = cls(
            option_type="call",
            strike=85,
            effective_time=START,
            averaging_start_time=START,
            expiry_time=end,
            realized_average=80,
        )
        for name in (analytic, "MonteCarloAsianEngine"):
            with subtests.test(engine=name, contract=cls.__name__):
                assert getattr(pricing, name)().price(option, context) == pytest.approx(
                    (average - 85) * math.exp(-0.08 * remaining), rel=0, abs=1e-10
                )


def test_forward_start_averages_and_sobol_uncertainty_use_the_deterministic_path(
    subtests,
):
    middle, end = START + timedelta(days=180), START + timedelta(days=365)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.08, dividend_yield=0.02, volatility=0
        ),
        spot_price=100,
        valuation_time=START,
    )
    future, lead = 185 / 365, 180 / 365
    arithmetic = (
        100 * math.exp(0.06 * lead) * math.expm1(0.06 * future) / (0.06 * future)
    )
    geometric = 100 * math.exp(0.06 * (lead + future / 2))
    for cls, analytic, average in (
        (
            ins.ArithmeticAveragePriceOption,
            "TurnbullWakemanArithmeticAveragePriceEngine",
            arithmetic,
        ),
        (
            ins.GeometricAveragePriceOption,
            "AnalyticGeometricAveragePriceEngine",
            geometric,
        ),
    ):
        option = cls(
            option_type="call",
            strike=90,
            effective_time=START,
            averaging_start_time=middle,
            expiry_time=end,
        )
        expected = (average - 90) * math.exp(-0.08)
        for name in (analytic, "MonteCarloAsianEngine"):
            with subtests.test(engine=name, contract=cls.__name__):
                assert getattr(pricing, name)().price(option, context) == pytest.approx(
                    expected, rel=0, abs=1e-10
                )
        estimate = pricing.MonteCarloAsianEngine(
            path_count=1, sampling="sobol", antithetic=True
        ).price_with_statistics(option, context)
        assert estimate.price == pytest.approx(expected, rel=0, abs=1e-10)
        assert estimate.standard_error is None
        assert estimate.confidence_interval is None


def test_future_structured_events_discount_coupons_and_settle_accumulated_units(
    subtests,
):
    middle, end = START + timedelta(days=180), START + timedelta(days=365)
    calendar = all_days_calendar()
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.1, dividend_yield=0, volatility=0
        ),
        spot_price=100,
        valuation_time=START,
    )
    note = ins.BinarySnowballOption(
        effective_time=START,
        expiry_time=end,
        maturity_coupon_rate=0.1,
        observations=ins.autocallable_observations(
            [middle, end], [105, 105], knock_out_coupon_rates=[0.1, 0.1]
        ),
        calendar=calendar,
    )
    for family in ("Quadrature", "FiniteDifference", "MonteCarlo"):
        with subtests.test(product="snowball", engine=family):
            assert getattr(pricing, family + "AutocallableEngine")().price(
                note, context
            ) == pytest.approx(
                (1 + 0.1 * 180 / 365) * math.exp(-0.1 * 180 / 365), rel=0, abs=1e-10
            )
    accumulator = ins.Accumulator(
        observation_clock_time=clock_time(),
        effective_time=START,
        expiry_time=end,
        strike=90,
        knock_out_level=105,
        daily_quantity=1,
        acceleration_factor=2,
        knock_out_observation_times=[middle],
        calendar=calendar,
    )
    expected = 180 * (100 * math.exp(0.1 * 180 / 365) - 90) * math.exp(-0.1 * 180 / 365)
    for family in ("Quadrature", "FiniteDifference", "MonteCarlo"):
        with subtests.test(product="accumulator", engine=family):
            assert getattr(pricing, family + "AccumulatorEngine")().price(
                accumulator, context
            ) == pytest.approx(expected, rel=0, abs=1e-8)


@pytest.mark.parametrize("family", ("Quadrature", "FiniteDifference", "MonteCarlo"))
def test_expiry_only_knock_in_ignores_earlier_recovered_breaches(family):
    end = START + timedelta(days=365)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.6, dividend_yield=0, volatility=0
        ),
        spot_price=70,
        valuation_time=START,
    )
    note = ins.SnowballOption(
        effective_time=START,
        expiry_time=end,
        maturity_coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation=KnockInObservation.at_expiry(),
        observations=ins.autocallable_observations(
            [START + timedelta(days=30), end],
            [300, 300],
            knock_out_coupon_rates=[0] * 2,
        ),
        calendar=all_days_calendar(),
    )
    assert getattr(pricing, family + "AutocallableEngine")().price(
        note, context
    ) == pytest.approx(1.1 * math.exp(-0.6), rel=0, abs=1e-10)


def test_deterministic_boundaries_and_zero_implied_volatility_are_explicit(subtests):
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0
        ),
        spot_price=100,
        valuation_time=START,
    )
    option = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    for name in (
        "AnalyticEuropeanEngine",
        "CoxRossRubinsteinVanillaEngine",
        "FiniteDifferenceVanillaEngine",
    ):
        engine = getattr(pricing, name)()
        with subtests.test(engine=name):
            result = engine.price_with_greeks(option, context, "all")
            assert result.status("delta") == "undefined"
            assert result.status("vega") == "undefined"
            assert (
                pricing.implied_volatility(
                    engine,
                    option,
                    context,
                    0,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
                == 0
            )
            digital = ins.CashOrNothingOption(
                option_type="call",
                strike=100,
                payout=10,
                effective_time=START,
                expiry_time=END,
            )
            assert pricing.AnalyticDigitalEngine().price(digital, context) == 0
    regular = option.replace(strike=90)
    result = pricing.AnalyticEuropeanEngine().price_with_greeks(regular, context, "all")
    assert result.delta == pytest.approx(1, rel=0, abs=1e-10)
    assert result.gamma == pytest.approx(0, rel=0, abs=1e-10)
    assert result.vega is None
    assert result.status("vega") == "undefined"
    for bad in (-0.1, math.nan, math.inf):
        with pytest.raises(InvalidInputError):
            context.model_parameters.replace(volatility=bad)


@pytest.mark.parametrize("sigma", (-0.1, math.nan, math.inf))
def test_negative_and_nonfinite_volatility_remain_invalid(sigma):
    with pytest.raises(InvalidInputError):
        BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=sigma
        )
