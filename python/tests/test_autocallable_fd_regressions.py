"""Autocallable finite-difference numerical regressions."""

from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    pricing,
)
from kiyosi.instruments import (
    BinarySnowballOption,
    CashOrNothingOption,
    EuropeanOption,
    KnockInObservation,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import numerical_greeks


@pytest.mark.parametrize("mode", ("every_trading_day", "at_expiry"))
def test_finite_difference_greek_workspaces_preserve_independent_prices(subtests, mode):
    from price_differences import price_differences

    start, end = datetime(2025, 1, 1), datetime(2025, 1, 13)
    engine = pricing.FiniteDifferenceAutocallableEngine(
        asset_step_count=80, time_step_count=80, asset_upper_boundary=400
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=-0.02, dividend_yield=0.01, volatility=0.3
        ),
        spot_price=100.25,
        valuation_time=datetime(2025, 1, 1, 12),
    )
    shifts = {"spot_shift": 0.25, "volatility_shift": 0.002, "rate_shift": 0.001}
    note = SnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0.06,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=60,
        knock_in_observation=(
            KnockInObservation.every_trading_day(clock_time())
            if mode == "every_trading_day"
            else KnockInObservation.at_expiry()
        ),
        barrier_state="no_event",
        effective_time=start,
        expiry_time=end,
        observations=autocallable_observations(
            [datetime(2025, 1, 3), end],
            [110, 105],
            knock_out_coupon_rates=[0.08, 0.08],
        ),
    )
    expected = price_differences(
        engine,
        note,
        context,
        ["vega", "rho"],
        **shifts,
    )
    actual = numerical_greeks(engine, note, context, greeks="all", **shifts)
    assert actual.price == expected.price
    assert actual.vega == expected.vega
    assert actual.rho == expected.rho
    for greek in (
        "speed",
        "vanna",
        "zomma",
        "charm",
        "color",
    ):
        with subtests.test(mode=mode, greek=greek):
            single = numerical_greeks(engine, note, context, greek, **shifts)
            assert getattr(single, greek) == getattr(actual, greek)


def test_automatic_domains_converge_to_volatile_terminal_portfolios(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=2
        ),
        spot_price=100,
        valuation_time=start,
    )
    binary = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=start,
        expiry_time=end,
        observations=autocallable_observations(
            [end], [100], knock_out_coupon_rates=[0.1]
        ),
    )
    digital = CashOrNothingOption(
        option_type="call",
        strike=100,
        payout=0.1,
        effective_time=start,
        expiry_time=end,
    )
    expected = 1 + pricing.AnalyticDigitalEngine().price(digital, context)
    for steps in (200, 2000):
        engine = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=steps, time_step_count=2000
        )
        with subtests.test(steps=steps):
            assert engine.price(binary, context) == pytest.approx(
                expected, rel=0, abs=0.0003
            )
    common = {
        "effective_time": start,
        "expiry_time": end,
        "knock_in_level": 100,
        "knock_in_observation": KnockInObservation.every_trading_day(clock_time()),
    }
    downside = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 60}
    put = lambda strike: pricing.AnalyticEuropeanEngine().price(
        EuropeanOption(
            option_type="put",
            strike=strike,
            effective_time=start,
            expiry_time=end,
        ),
        context,
    )
    expected_downside = expected - (put(100) - put(60)) / 100
    for note, value in (
        (
            SnowballOption(
                calendar=weekdays_calendar(),
                **common,
                **downside,
                maturity_coupon_rate=0,
                observations=binary.observations,
            ),
            expected_downside,
        ),
        (
            PhoenixOption(
                calendar=weekdays_calendar(),
                **common,
                **downside,
                coupon_per_period=0.1,
                observations=autocallable_observations(
                    [end], [100], coupon_barrier_levels=[100]
                ),
            ),
            expected_downside,
        ),
        (
            TernarySnowballOption(
                calendar=weekdays_calendar(),
                **common,
                maturity_coupon_rate=0.1,
                minimum_coupon_rate=0,
                observations=binary.observations,
            ),
            expected,
        ),
    ):
        with subtests.test(product=type(note).__name__):
            assert engine.price(note, context) == pytest.approx(
                value, rel=0, abs=0.0003
            )
    result = engine.price_with_greeks(
        binary,
        context,
        ["delta", "gamma", "vega", "rho"],
    )
    reference = pricing.AnalyticDigitalEngine().price_with_greeks(
        digital,
        context,
        ["delta", "gamma", "vega", "rho"],
    )
    for greek in (
        "delta",
        "gamma",
        "vega",
        "rho",
    ):
        value = getattr(reference, greek) - (1 if greek == "rho" else 0)
        assert getattr(result, greek) == pytest.approx(
            value, rel=0, abs=abs(value) * 0.02 + 1e-8
        )
    quote = engine.price(binary, context)
    fixed = pricing.FiniteDifferenceAutocallableEngine(
        asset_step_count=2000, time_step_count=2000, asset_upper_boundary=1e6
    )
    delta = (
        fixed.price(
            binary,
            context.replace(spot_price=100.1),
        )
        - fixed.price(
            binary,
            context.replace(spot_price=99.9),
        )
    ) / 0.2
    assert fixed.price_with_greeks(binary, context, "delta").delta == pytest.approx(
        delta, rel=0, abs=abs(delta) * 0.02
    )
    volatility = pricing.implied_volatility(
        engine,
        binary,
        context,
        quote,
        lower_bound=1.5,
        upper_bound=2.5,
        price_tolerance=1e-10,
        parameter_tolerance=1e-10,
    )
    recovered = context.replace(
        model_parameters=context.model_parameters.replace(volatility=volatility)
    )
    assert engine.price(binary, recovered) == pytest.approx(quote, rel=0, abs=1e-8)
    explicit = pricing.FiniteDifferenceAutocallableEngine(
        asset_step_count=200, time_step_count=2000, scheme="explicit_euler"
    )
    assert explicit.price(binary, context) == pytest.approx(expected, rel=0, abs=0.0003)


def test_finite_difference_volatility_trials_refresh_operators_and_isolate_requests(
    subtests,
):
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 13)
    observation_times = [datetime(2025, 1, 3), datetime(2025, 1, 8), end]
    knock_out_levels = [101, 100, 99]
    common = {
        "effective_time": start,
        "expiry_time": end,
        "barrier_state": "no_event",
    }
    coupons = {
        "maturity_coupon_rate": 0.06,
        "observations": autocallable_observations(
            observation_times, knock_out_levels, knock_out_coupon_rates=[0.08] * 3
        ),
    }

    def context(volatility):
        return PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=volatility
            ),
            spot_price=100.25,
            valuation_time=datetime(2025, 1, 1, 12),
        )

    base = context(0.27)
    bounds = {"lower_bound": 0.1, "upper_bound": 0.4, "price_tolerance": 1e-12}
    for mode in ("at_expiry", "every_trading_day"):
        knock_in = {
            "knock_in_level": 99,
            "knock_in_observation": (
                KnockInObservation.every_trading_day(clock_time())
                if mode == "every_trading_day"
                else KnockInObservation.at_expiry()
            ),
        }
        cases = (
            (
                PhoenixOption,
                dict(
                    **knock_in,
                    initial_spot=100,
                    upper_strike=100,
                    lower_strike=60,
                    coupon_per_period=0.08,
                    observations=autocallable_observations(
                        observation_times,
                        knock_out_levels,
                        coupon_barrier_levels=[100, 99, 98],
                    ),
                ),
                pricing.FiniteDifferenceAutocallableEngine,
            ),
            (
                SnowballOption,
                dict(
                    **knock_in,
                    **coupons,
                    initial_spot=100,
                    upper_strike=100,
                    lower_strike=60,
                ),
                pricing.FiniteDifferenceAutocallableEngine,
            ),
            (
                BinarySnowballOption,
                coupons,
                pricing.FiniteDifferenceAutocallableEngine,
            ),
            (
                TernarySnowballOption,
                dict(
                    **knock_in,
                    **coupons,
                    minimum_coupon_rate=0.02,
                ),
                pricing.FiniteDifferenceAutocallableEngine,
            ),
        )
        for kind, terms, engine_type in cases:
            note = kind(calendar=weekdays_calendar(), **common, **terms)
            for scheme in (
                "explicit_euler",
                "implicit_euler",
                "crank_nicolson",
                "rannacher",
            ):
                with subtests.test(kind=kind, mode=mode, scheme=scheme):
                    engine = engine_type(
                        asset_step_count=32, time_step_count=64, scheme=scheme
                    )
                    quotes = [
                        engine.price(
                            note,
                            context(volatility),
                        )
                        for volatility in (0.17, 0.33)
                    ]

                    def solve(quote, *, engine=engine, note=note):
                        return pricing.implied_volatility(
                            engine,
                            note,
                            base,
                            quote,
                            **{"lower_bound": 0.0, "upper_bound": 4.0, **bounds},
                        )

                    expected = [solve(quote) for quote in quotes]
                    for quote, volatility in zip(quotes, expected):
                        assert engine.price(
                            note,
                            context(volatility),
                        ) == pytest.approx(quote, rel=0, abs=1e-10)
                    with pytest.raises(RootNotBracketedError) as error:
                        solve(1e10)
                    assert type(error.value) is RootNotBracketedError
                    with ThreadPoolExecutor(max_workers=2) as pool:
                        recovered = list(pool.map(solve, quotes))
                    assert recovered == expected


@pytest.mark.parametrize("mode", ("at_expiry", "every_trading_day"))
def test_coupon_endpoint_batches_preserve_layer_changes_and_isolation(subtests, mode):
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 13)
    observation_times = [datetime(2025, 1, 7), end]
    knock_out_levels = [110, 110]
    common = {
        "effective_time": start,
        "expiry_time": end,
        "barrier_state": "no_event",
    }
    coupons = {
        "maturity_coupon_rate": 0.073,
        "observations": autocallable_observations(
            observation_times,
            knock_out_levels,
            knock_out_coupon_rates=[0.073, 0.083],
        ),
    }
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0, volatility=0.3
        ),
        spot_price=100.25,
        valuation_time=datetime(2025, 1, 1, 12),
    )
    knock_in = {
        "knock_in_level": 99,
        "knock_in_observation": (
            KnockInObservation.every_trading_day(clock_time())
            if mode == "every_trading_day"
            else KnockInObservation.at_expiry()
        ),
    }
    cases = (
        (
            PhoenixOption,
            dict(
                **knock_in,
                initial_spot=100,
                upper_strike=100,
                lower_strike=60,
                coupon_per_period=0.073,
                observations=autocallable_observations(
                    observation_times,
                    knock_out_levels,
                    coupon_barrier_levels=[90, 90],
                ),
            ),
            pricing.FiniteDifferenceAutocallableEngine,
        ),
        (
            SnowballOption,
            dict(
                **knock_in,
                **coupons,
                initial_spot=100,
                upper_strike=100,
                lower_strike=100,
            ),
            pricing.FiniteDifferenceAutocallableEngine,
        ),
        (
            BinarySnowballOption,
            coupons,
            pricing.FiniteDifferenceAutocallableEngine,
        ),
        (
            TernarySnowballOption,
            dict(
                **knock_in,
                **coupons,
                minimum_coupon_rate=0,
            ),
            pricing.FiniteDifferenceAutocallableEngine,
        ),
    )
    for kind, terms, engine_type in cases:
        note = kind(calendar=weekdays_calendar(), **common, **terms)
        engines = [
            (
                engine_type(
                    asset_step_count=16,
                    time_step_count=8,
                    scheme=scheme,
                    asset_upper_boundary=400,
                ),
                scheme,
            )
            for scheme in (
                "explicit_euler",
                "implicit_euler",
                "crank_nicolson",
                "rannacher",
            )
        ]
        engines.extend(
            (
                pricing.QuadratureAutocallableEngine(asset_step_count=steps),
                f"quadrature_{steps}",
            )
            for steps in (17, 128)
        )
        engines.append(
            (
                pricing.MonteCarloAutocallableEngine(path_count=1024, seed=42),
                "monte_carlo",
            )
        )
        for engine, scheme in engines:
            quote = engine.price(note, context)
            conventions = (
                ({},)
                if kind is PhoenixOption
                else (
                    {"quote_convention": "shift_maturity_coupon"},
                    {"quote_convention": "preserve_maturity_coupon"},
                )
            )
            for convention in conventions:
                alternate_terms = dict(terms)
                if kind is PhoenixOption:
                    alternate_terms["coupon_per_period"] = 0.12
                else:
                    alternate_terms["observations"] = autocallable_observations(
                        observation_times,
                        knock_out_levels,
                        knock_out_coupon_rates=[0.12, 0.13],
                    )
                    if convention["quote_convention"] == "shift_maturity_coupon":
                        alternate_terms["maturity_coupon_rate"] = 0.12
                alternate_quote = engine.price(
                    kind(
                        calendar=weekdays_calendar(),
                        **common,
                        **alternate_terms,
                    ),
                    context,
                )

                def solve(
                    observed_price,
                    *,
                    convention=convention,
                    engine=engine,
                    note=note,
                ):
                    # Resolve brackets finely enough for the price tolerance across coupon units.
                    return (
                        pricing.implied_phoenix_coupon
                        if isinstance(note, PhoenixOption)
                        else pricing.implied_snowball_coupon
                    )(
                        engine,
                        note,
                        context,
                        observed_price,
                        lower_bound=0,
                        upper_bound=0.2,
                        price_tolerance=1e-14,
                        parameter_tolerance=1e-15,
                        **convention,
                    )

                with subtests.test(
                    kind=kind, mode=mode, scheme=scheme, convention=convention
                ):
                    with ThreadPoolExecutor(max_workers=2) as pool:
                        results = list(pool.map(solve, (quote, alternate_quote)))
                    field = (
                        "coupon_per_period"
                        if isinstance(note, PhoenixOption)
                        else "first_observation_coupon"
                    )
                    solved_coupons = [result[field] for result in results]
                    assert solved_coupons[0] == pytest.approx(0.073, rel=0, abs=1e-9)
                    assert solved_coupons[1] == pytest.approx(0.12, rel=0, abs=1e-9)
                    for result, target in zip(
                        results, (quote, alternate_quote), strict=True
                    ):
                        repriced = note.with_quoted_coupon(**result)
                        assert engine.price(repriced, context) == pytest.approx(
                            target, rel=0, abs=1e-12
                        )
                    with pytest.raises(InvalidInputError) as error:
                        (
                            pricing.implied_phoenix_coupon
                            if isinstance(note, PhoenixOption)
                            else pricing.implied_snowball_coupon
                        )(
                            engine,
                            note,
                            context,
                            quote,
                            lower_bound=0.2,
                            upper_bound=0,
                            **convention,
                        )
                    assert type(error.value) is InvalidInputError
                    assert solve(quote) == results[0]


def test_autocallable_requests_revalidate_contracts_and_calendars(subtests):
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 13)
    dates = [start + timedelta(days=index) for index in range(1, 13)]
    observation_times = dates
    knock_out_levels = [110] * len(dates)
    common = {
        "effective_time": start,
        "expiry_time": end,
        "calendar": all_days_calendar(),
    }
    coupons = {
        "maturity_coupon_rate": 0.06,
        "observations": autocallable_observations(
            observation_times,
            knock_out_levels,
            knock_out_coupon_rates=[0.08] * len(dates),
        ),
    }
    downside = {
        "initial_spot": 100,
        "knock_in_level": 99,
        "upper_strike": 100,
        "lower_strike": 60,
        "knock_in_observation": KnockInObservation.every_trading_day(clock_time()),
    }
    cases = (
        (BinarySnowballOption, coupons),
        (SnowballOption, {**coupons, **downside}),
        (
            TernarySnowballOption,
            {
                **coupons,
                "knock_in_level": 99,
                "minimum_coupon_rate": 0.02,
                "knock_in_observation": KnockInObservation.every_trading_day(
                    clock_time()
                ),
            },
        ),
        (
            PhoenixOption,
            {
                **downside,
                "coupon_per_period": 0.08,
                "observations": autocallable_observations(
                    observation_times,
                    knock_out_levels,
                    coupon_barrier_levels=[100] * len(dates),
                ),
            },
        ),
    )

    def context(rate=0.04, volatility=0.2):
        return PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=rate, dividend_yield=0.01, volatility=volatility
            ),
            spot_price=100.25,
            valuation_time=datetime(2025, 1, 12, 12),
        )

    base = context()
    for option_type, terms in cases:
        note = option_type(
            **common,
            **terms,
            barrier_state="no_event",
        )
        missing_history = option_type(**common, **terms)
        engines = (
            pricing.FiniteDifferenceAutocallableEngine(
                asset_step_count=80, time_step_count=1
            ),
            pricing.QuadratureAutocallableEngine(asset_step_count=16),
        )
        for engine in engines:
            with subtests.test(engine=type(engine).__name__):
                shifts = {"volatility_shift": 0.01, "rate_shift": 0.01}
                result = numerical_greeks(
                    engine,
                    note,
                    base,
                    ["vega", "rho"],
                    **shifts,
                )
                assert result.price == engine.price(note, base)
                vol_shift = (0.2 + 0.01) - 0.2
                assert result.vega == (
                    engine.price(
                        note,
                        context(volatility=0.2 + vol_shift),
                    )
                    - engine.price(
                        note,
                        context(volatility=0.2 - vol_shift),
                    )
                ) / (2 * vol_shift)
                rate_shift = (0.04 + 0.01) - 0.04
                assert result.rho == (
                    engine.price(
                        note,
                        context(rate=0.04 + rate_shift),
                    )
                    - engine.price(
                        note,
                        context(rate=0.04 - rate_shift),
                    )
                ) / (2 * rate_shift)
                with pytest.raises(InvalidInputError):
                    note.replace(calendar=weekdays_calendar())
                for option, shifted, expected_error in (
                    (missing_history, base, MissingHistoryError),
                ):
                    with pytest.raises(expected_error) as error:
                        engine.price_with_greeks(
                            option,
                            shifted,
                            [
                                "vega",
                                "rho",
                            ],
                        )
                    assert type(error.value) is expected_error
                with ThreadPoolExecutor(max_workers=2) as pool:
                    results = list(
                        pool.map(
                            lambda _, *, engine=engine, note=note, shifts=shifts: (
                                numerical_greeks(
                                    engine,
                                    note,
                                    base,
                                    [
                                        "vega",
                                        "rho",
                                    ],
                                    **shifts,
                                )
                            ),
                            range(2),
                        )
                    )
                for recovered in results:
                    assert (
                        (
                            recovered.price,
                            recovered.vega,
                            recovered.rho,
                        )
                    ) == (
                        (
                            result.price,
                            result.vega,
                            result.rho,
                        )
                    )


def test_irrelevant_knock_in_events_preserve_spot_and_time_greeks(subtests):
    import math

    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    note = SnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        initial_spot=100,
        knock_in_level=75,
        upper_strike=100,
        lower_strike=100,
        barrier_state="no_event",
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        effective_time=start,
        expiry_time=end,
        observations=autocallable_observations(
            [end], [110], knock_out_coupon_rates=[0]
        ),
    )
    for engine in (pricing.FiniteDifferenceAutocallableEngine(),):
        for time in (
            datetime(2025, 1, 1),
            datetime(2025, 1, 2, 12),
        ):
            with subtests.test(engine=type(engine).__name__, time=time):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                    ),
                    spot_price=75,
                    valuation_time=time,
                )
                result = engine.price_with_greeks(
                    note,
                    context,
                    ["delta", "gamma", "theta", "charm", "color"],
                )
                years = (datetime(2026, 1, 1) - time).total_seconds() / (365 * 86400)
                assert result.price == pytest.approx(
                    math.exp(-0.04 * years), rel=0, abs=1e-12
                )
                assert ((result.delta, result.gamma, result.charm, result.color)) == (
                    (0, 0, 0, 0)
                )
                assert result.theta is not None
                assert result.theta == pytest.approx(
                    0.04 * result.price, rel=0, abs=0.00000365
                )


def test_completed_knock_in_preserves_time_greeks_without_hiding_live_events(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=75,
        valuation_time=datetime(2025, 1, 2, 12),
    )
    for engine in (pricing.FiniteDifferenceAutocallableEngine(),):
        for state in ("no_event", "knocked_in"):
            with subtests.test(engine=type(engine).__name__, state=state):
                note = SnowballOption(
                    calendar=weekdays_calendar(),
                    maturity_coupon_rate=0.06,
                    initial_spot=100,
                    knock_in_level=75,
                    upper_strike=100,
                    lower_strike=60,
                    barrier_state=state,
                    effective_time=start,
                    expiry_time=end,
                    knock_in_observation=KnockInObservation.every_trading_day(
                        clock_time()
                    ),
                    observations=autocallable_observations(
                        [end], [110], knock_out_coupon_rates=[0.08]
                    ),
                )
                result = engine.price_with_greeks(
                    note,
                    context,
                    ["theta", "charm", "color"],
                )
                for value in (result.theta, result.charm, result.color):
                    assert value is not None


@pytest.mark.parametrize(
    "scheme", ("crank_nicolson", "rannacher", "implicit_euler", "explicit_euler")
)
def test_expiry_only_knock_in_matches_analytic_terminal_portfolios(subtests, scheme):
    import math

    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    observation_times = [end]
    knock_out_levels = [110]
    common = {
        "knock_in_level": 100,
        "knock_in_observation": KnockInObservation.at_expiry(),
        "effective_time": start,
        "expiry_time": end,
    }
    snowball = SnowballOption(
        calendar=weekdays_calendar(),
        **common,
        maturity_coupon_rate=0.06,
        initial_spot=100,
        upper_strike=100,
        lower_strike=60,
        observations=autocallable_observations(
            observation_times, knock_out_levels, knock_out_coupon_rates=[0.06]
        ),
    )
    phoenix = PhoenixOption(
        calendar=weekdays_calendar(),
        **common,
        coupon_per_period=0.08,
        initial_spot=100,
        upper_strike=100,
        lower_strike=60,
        observations=autocallable_observations(
            observation_times, knock_out_levels, coupon_barrier_levels=[0]
        ),
    )
    ternary = TernarySnowballOption(
        calendar=weekdays_calendar(),
        **common,
        maturity_coupon_rate=0.06,
        minimum_coupon_rate=0.02,
        observations=autocallable_observations(
            observation_times, knock_out_levels, knock_out_coupon_rates=[0.06]
        ),
    )
    put_prices = [
        pricing.AnalyticEuropeanEngine().price(
            EuropeanOption(
                option_type="put",
                strike=strike,
                effective_time=start,
                expiry_time=end,
            ),
            context,
        )
        for strike in (100, 60)
    ]
    discount = math.exp(-0.04)
    downside = discount - (put_prices[0] - put_prices[1]) / 100
    digital = pricing.AnalyticDigitalEngine().price(
        CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=1,
            effective_time=start,
            expiry_time=end,
        ),
        context,
    )
    cases = (
        (
            snowball,
            pricing.FiniteDifferenceAutocallableEngine,
            downside + 0.06 * digital,
        ),
        (
            phoenix,
            pricing.FiniteDifferenceAutocallableEngine,
            downside + 0.08 * discount,
        ),
        (
            ternary,
            pricing.FiniteDifferenceAutocallableEngine,
            1.02 * discount + 0.04 * digital,
        ),
    )
    for note, engine_type, expected in cases:
        with subtests.test(scheme=scheme, note=type(note).__name__):
            actual = engine_type(
                asset_step_count=200,
                time_step_count=2000,
                scheme=scheme,
                asset_upper_boundary=400,
            ).price(note, context)
            assert actual == pytest.approx(expected, rel=0, abs=0.002)


def test_protected_snowball_uses_observation_paths_with_stochastic_knock_out(subtests):
    start, middle, end = (
        datetime(2025, 1, 1),
        datetime(2025, 7, 1),
        datetime(2026, 1, 1),
    )
    terms = {
        "calendar": all_days_calendar(),
        "maturity_coupon_rate": 0,
        "initial_spot": 100,
        "knock_in_level": 75,
        "upper_strike": 100,
        "lower_strike": 100,
        "effective_time": start,
        "expiry_time": end,
        "observations": autocallable_observations(
            [middle, end], [110, 104], knock_out_coupon_rates=[0.08, 0.08]
        ),
    }
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    for engine in (
        pricing.FiniteDifferenceAutocallableEngine(scheme="rannacher"),
        pricing.FiniteDifferenceAutocallableEngine(scheme="implicit_euler"),
    ):
        with subtests.test(engine=type(engine).__name__):
            assert engine.price(
                SnowballOption(
                    **terms,
                    knock_in_observation=KnockInObservation.every_trading_day(
                        clock_time()
                    ),
                ),
                context,
            ) == engine.price(
                SnowballOption(
                    **terms,
                    knock_in_observation=KnockInObservation.at_expiry(),
                ),
                context,
            )
    explicit = pricing.FiniteDifferenceAutocallableEngine(
        asset_step_count=80,
        time_step_count=200,
        scheme="explicit_euler",
        asset_upper_boundary=400,
    )
    actual = explicit.price(
        SnowballOption(
            **terms,
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        ),
        context,
    )
    reference = pricing.FiniteDifferenceAutocallableEngine(
        asset_step_count=80,
        time_step_count=2000,
        scheme="explicit_euler",
        asset_upper_boundary=400,
    ).price(
        SnowballOption(
            **terms,
            knock_in_observation=KnockInObservation.at_expiry(),
        ),
        context,
    )
    assert actual == pytest.approx(reference, rel=0, abs=0.0002)
    with pytest.raises(InvalidInputError) as error:
        explicit.price(
            SnowballOption(
                **terms,
                knock_in_observation=KnockInObservation.at_expiry(),
            ),
            context,
        )
    assert type(error.value) is InvalidInputError


def test_fixed_finite_difference_autocallables_retain_rate_sensitivity():
    import math

    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    note = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=start,
        expiry_time=end,
        observations=autocallable_observations(
            [end], [110], knock_out_coupon_rates=[0]
        ),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    result = pricing.FiniteDifferenceAutocallableEngine().price_with_greeks(
        note,
        context,
        ["delta", "gamma", "vega", "rho"],
    )
    assert result.price == math.exp(-0.04)
    assert ((result.delta, result.gamma, result.vega)) == ((0, 0, 0))
    assert result.rho == pytest.approx((-math.exp(-0.04)), rel=0, abs=0.000001)
