"""Snowball variants through the installed Rust public API."""

import inspect
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
    instruments,
)
from kiyosi.instruments import (
    AutocallableObservation,
    BinarySnowballOption,
    KnockInObservation,
    SnowballOption,
    TernarySnowballOption,
)
from kiyosi.market import RateTermStructure, weekdays_calendar
from kiyosi.pricing import (
    MonteCarloAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_snowball_coupon,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_decimal_equal, assert_rejection
from test_snowball import END, START, autocallable_observations, market, structured


def test_european_snowball_signature_defaults_can_be_forwarded():
    factory = instruments.european_snowball
    signature = inspect.signature(factory)
    assert "knock_in_observation_clock_time" not in signature.parameters
    fields = {
        "coupon_rate": 0.12,
        "initial_spot": 100,
        "knock_in_level": 80,
        "knock_out_level": 110,
        "autocall_times": [END],
        "effective_time": START,
        "expiry_time": END,
    }
    expected = factory(**fields)
    bound = signature.bind(**fields)
    bound.apply_defaults()
    assert factory(**bound.arguments) == expected
    assert expected.knock_in_observation == KnockInObservation.at_expiry()
    assert expected.knock_in_observation.kind == "at_expiry"


def test_quadrature_covers_drift_into_future_knock_out():
    end = datetime(2026, 1, 1)
    note = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=START,
        expiry_time=end,
        autocall_observations=autocallable_observations(
            [datetime(2025, 6, 30), end],
            [110, 110],
            knock_out_coupon_rates=[0.2, 0.2],
        ),
    )
    context = market(106, volatility=0.001, rate=0.05, dividend=0)
    engine = QuadratureAutocallableEngine()
    expected = 1.2 * math.exp(-0.05)
    assert engine.price(note, context) == pytest.approx(expected, rel=0, abs=1e-7)
    assert implied_snowball_coupon(
        engine,
        note,
        context,
        expected,
        quote_convention="preserve_maturity_coupon",
    )["first_observation_coupon"] == pytest.approx(0.2, rel=0, abs=1e-7)


def test_quadrature_covers_drift_below_future_knock_out():
    end = datetime(2026, 1, 1)
    note = SnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        effective_time=START,
        expiry_time=end,
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        autocall_observations=autocallable_observations(
            [end], [110], knock_out_coupon_rates=[0.2]
        ),
    )
    context = market(115, volatility=0.005, rate=0, dividend=0.05)
    d2 = (math.log(115 / 110) - 0.05 - 0.5 * 0.005**2) / 0.005
    expected = 1 + 0.1 * math.erfc(-d2 / math.sqrt(2))
    assert QuadratureAutocallableEngine(asset_step_count=2048).price(
        note, context
    ) == pytest.approx(expected, rel=0, abs=1e-4)


@pytest.mark.parametrize("kind", (BinarySnowballOption, TernarySnowballOption))
def test_history_replacement_preserves_terms_and_rejects_binary_knock_in(
    subtests, kind
):
    note = make_fixed(kind)
    engine = QuadratureAutocallableEngine()
    states = (None, "no_event")
    for state in states:
        with subtests.test(kind=kind.__name__, state=state):
            replaced = note.replace(
                barrier_state=state,
            )
            assert replaced == make_fixed(kind, barrier_state=state)
            assert replaced.replace(barrier_state=None) == note
    for state in ("knocked_in", "knocked_out"):
        with subtests.test(kind=kind.__name__, state=state):
            assert_rejection(
                InvalidInputError,
                lambda state=state: engine.price(
                    note.replace(barrier_state=state), market(100, START)
                ),
            )
    assert note == make_fixed(kind)


def test_fixed_terminal_regions_and_no_binary_monitoring():
    common = {
        "maturity_coupon_rate": 0.06,
        "principal_redemption_ratio": 0.5,
        "effective_time": START,
        "expiry_time": END,
        "autocall_observations": autocallable_observations(
            [END], [110], knock_out_coupon_rates=[0.08]
        ),
    }
    binary = BinarySnowballOption(calendar=weekdays_calendar(), **common)
    ternary = TernarySnowballOption(
        calendar=weekdays_calendar(),
        **common,
        minimum_coupon_rate=-0.02,
        knock_in_level=80,
        knock_in_observation=KnockInObservation.at_expiry(),
    )
    engine = QuadratureAutocallableEngine()
    valuation = datetime(2025, 2, 3)
    time = (END - valuation).days / 365
    d2 = (math.log(80 / 110) + (0.04 - 0.01 - 0.3**2 / 2) * time) / (
        0.3 * math.sqrt(time)
    )
    knock_out_probability = math.erfc(-d2 / math.sqrt(2)) / 2
    expected = math.exp(-0.04 * time) * (
        0.5 + (0.06 + 0.02 * knock_out_probability) * 90 / 365
    )
    assert engine.price(binary, market(80, valuation)) == pytest.approx(
        expected, rel=0, abs=1e-12
    )
    for spot in (79.999, 80, 109, 110):
        for note in (binary, ternary):
            coupon = (
                0.08
                if spot >= 110
                else -0.02
                if spot < 80 and note is ternary
                else 0.06
            )
            assert engine.price(note, market(spot, END)) == 0.5 + coupon * 90 / 365


def make_fixed(kind, **changes):
    fields = {
        "maturity_coupon_rate": 0.06,
        "effective_time": START,
        "expiry_time": END,
        "autocall_observations": autocallable_observations(
            [END], [110], knock_out_coupon_rates=[0.08]
        ),
    }
    if kind is TernarySnowballOption:
        fields.update(
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            knock_in_observation=KnockInObservation.at_expiry(),
        )
    fields.update(changes)
    return kind(calendar=weekdays_calendar(), **fields)


def test_existing_quantlib_terminal_portfolios(subtests):
    engine = QuadratureAutocallableEngine()
    for name, kind, inputs in structured.scenarios():
        if kind not in ("BinarySnowballOption", "TernarySnowballOption"):
            continue
        with subtests.test(name=name):
            fields = {
                "effective_time": datetime.fromisoformat(inputs["effective_time"]),
                "expiry_time": datetime.fromisoformat(inputs["expiry_time"]),
                "barrier_state": inputs["history"],
                "principal_redemption_ratio": inputs["principal_redemption_ratio"],
                "maturity_coupon_rate": inputs["maturity_coupon_rate"],
                "autocall_observations": autocallable_observations(
                    [datetime.fromisoformat(inputs["expiry_time"])],
                    [110],
                    knock_out_coupon_rates=[inputs["coupon_rate"]],
                ),
            }
            if kind == "TernarySnowballOption":
                fields.update(
                    minimum_coupon_rate=inputs["minimum_coupon_rate"],
                    knock_in_level=inputs["knock_in_level"],
                    knock_in_observation=(
                        KnockInObservation.every_trading_day(clock_time())
                        if inputs["knock_in_mode"] == "every_trading_day"
                        else KnockInObservation.at_expiry()
                    ),
                )
            note = getattr(instruments, kind)(calendar=weekdays_calendar(), **fields)
            ctx = market(
                inputs["spot"],
                datetime.fromisoformat(inputs["valuation"]),
                volatility=inputs["volatility"],
                rate=inputs["rate"],
            )
            assert engine.price(note, ctx) == pytest.approx(
                structured.price(kind, inputs),
                rel=0,
                abs=1e-10,
            )


def test_all_eight_presets_match_explicit_terms_prices_and_analysis(subtests):
    dates = [datetime(2025, 2, 3), datetime(2025, 3, 3), END]
    common = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "autocall_times": dates,
        "effective_time": START,
        "expiry_time": END,
        "principal_redemption_ratio": 0.5,
        "barrier_state": "no_event",
    }
    cases = [
        (
            "standard_snowball",
            {"coupon_rate": 0.125, "knock_out_level": 110},
            [110] * 3,
            [0.125] * 3,
            0.125,
            100,
            0,
            "every_trading_day",
        ),
        (
            "european_snowball",
            {"coupon_rate": 0.125, "knock_out_level": 110},
            [110] * 3,
            [0.125] * 3,
            0.125,
            100,
            0,
            "at_expiry",
        ),
        (
            "step_down_snowball",
            {
                "coupon_rate": 0.125,
                "initial_knock_out_level": 110,
                "knock_out_level_decrement": 2,
            },
            [110, 108, 106],
            [0.125] * 3,
            0.125,
            100,
            0,
            "every_trading_day",
        ),
        (
            "both_down_snowball",
            {
                "initial_coupon_rate": 0.125,
                "initial_knock_out_level": 110,
                "knock_out_level_decrement": 2,
                "coupon_rate_decrement": 0.03125,
            },
            [110, 108, 106],
            [0.125, 0.09375, 0.0625],
            0.0625,
            100,
            0,
            "every_trading_day",
        ),
        (
            "dual_coupon_snowball",
            {
                "knock_out_coupon_rate": 0.125,
                "knock_out_level": 110,
                "maturity_coupon_rate": -0.02,
            },
            [110] * 3,
            [0.125] * 3,
            -0.02,
            100,
            0,
            "every_trading_day",
        ),
        (
            "parachute_snowball",
            {
                "coupon_rate": 0.125,
                "knock_out_level": 110,
                "final_knock_out_level": 95,
            },
            [110, 110, 95],
            [0.125] * 3,
            0.125,
            100,
            0,
            "every_trading_day",
        ),
        (
            "otm_snowball",
            {"coupon_rate": 0.125, "knock_out_level": 110, "upper_strike": 90},
            [110] * 3,
            [0.125] * 3,
            0.125,
            90,
            0,
            "every_trading_day",
        ),
        (
            "loss_capped_snowball",
            {"coupon_rate": 0.125, "knock_out_level": 110, "lower_strike": 60},
            [110] * 3,
            [0.125] * 3,
            0.125,
            100,
            60,
            "every_trading_day",
        ),
    ]
    engine = QuadratureAutocallableEngine(asset_step_count=128)
    ctx = market(100, datetime(2025, 1, 15))
    for name, extra, levels, coupons, maturity, upper, lower, mode in cases:
        with subtests.test(preset=name):
            actual = getattr(instruments, name)(
                **(
                    {"knock_in_observation_clock_time": clock_time()}
                    if mode == "every_trading_day"
                    else {}
                ),
                **common,
                **extra,
            )
            explicit = SnowballOption(
                calendar=weekdays_calendar(),
                maturity_coupon_rate=maturity,
                initial_spot=100,
                knock_in_level=80,
                upper_strike=upper,
                lower_strike=lower,
                knock_in_observation=(
                    KnockInObservation.every_trading_day(clock_time())
                    if mode == "every_trading_day"
                    else KnockInObservation.at_expiry()
                ),
                barrier_state="no_event",
                principal_redemption_ratio=0.5,
                effective_time=START,
                expiry_time=END,
                autocall_observations=autocallable_observations(
                    dates, levels, knock_out_coupon_rates=coupons
                ),
            )
            assert actual == explicit
            assert engine.price(actual, ctx) == engine.price(explicit, ctx)
            mc = MonteCarloAutocallableEngine(path_count=128, seed=42)
            assert mc.price(actual, ctx) == mc.price(explicit, ctx)
            for note in (actual, explicit):
                result = numerical_greeks(
                    engine,
                    note,
                    ctx,
                    ["delta", "vega"],
                    spot_shift=0.25,
                )
                assert result.delta is not None
                assert result.vega is not None
            if mode == "at_expiry":
                for note in (actual, explicit):
                    assert_rejection(
                        InvalidInputError,
                        lambda note=note: engine.price(
                            note.replace(barrier_state="knocked_in"), market(100, START)
                        ),
                    )
            else:
                assert actual.replace(
                    barrier_state="knocked_in",
                ) == explicit.replace(
                    barrier_state="knocked_in",
                )
            assert (
                eval(
                    repr(actual),
                    vars(instruments)
                    | {
                        "datetime": __import__("datetime"),
                        "weekdays_calendar": weekdays_calendar,
                    },
                )
                == actual
            )
    defaults = instruments.standard_snowball(
        knock_in_observation_clock_time=clock_time(),
        coupon_rate=0.08,
        initial_spot=100,
        knock_in_level=80,
        knock_out_level=110,
        autocall_times=[END],
        effective_time=START,
        expiry_time=END,
    )
    assert defaults.principal_redemption_ratio == 1
    assert defaults.barrier_state is None


def test_native_representations_validation_timing_and_ownership():
    engine = QuadratureAutocallableEngine()
    for kind in (BinarySnowballOption, TernarySnowballOption):
        note = make_fixed(kind)
        assert note.principal_redemption_ratio == 1
        assert note.barrier_state is None
        assert (
            eval(
                repr(note),
                vars(instruments)
                | {
                    "datetime": __import__("datetime"),
                    "weekdays_calendar": weekdays_calendar,
                },
            )
            == note
        )
        records = list(note.autocall_observations)
        records.clear()
        assert len(note.autocall_observations) == 1
        for field in ("maturity_coupon_rate", "principal_redemption_ratio"):
            for value in (True, "0.1", object()):
                with pytest.raises(TypeError):
                    make_fixed(kind, **{field: value})
            for value in (math.nan, math.inf):
                assert_rejection(InvalidInputError, make_fixed, kind, **{field: value})
        for records, expected_error in (
            ([], InvalidInputError),
            (
                [AutocallableObservation.phoenix(END, 110, coupon_barrier_level=1)],
                InvalidInputError,
            ),
        ):
            assert_rejection(
                expected_error, make_fixed, kind, autocall_observations=records
            )
        with pytest.raises(TypeError):
            make_fixed(kind, autocall_observations=[object()])
        with pytest.raises(TypeError):
            make_fixed(kind, effective_time=datetime(2025, 1, 1).date())
        assert_rejection(
            InvalidInputError, make_fixed, kind, expiry_time=datetime(2024, 12, 31)
        )
        assert_rejection(
            InvalidInputError,
            make_fixed,
            kind,
            autocall_observations=[],
            maturity_coupon_rate=math.nan,
        )
        with pytest.raises(ValueError):
            note.replace(
                barrier_state="unknown",
            )
        with pytest.raises(TypeError):
            note.replace(
                barrier_state=True,
            )
        assert note.barrier_state is None
        assert_rejection(
            InvalidInputError,
            lambda note=note: engine.price(
                note.replace(barrier_state="knocked_out"), market(100, START)
            ),
        )
        assert_rejection(
            InvalidInputError,
            engine.price,
            note,
            market(100, datetime(2025, 4, 2)),
        )
    binary = make_fixed(BinarySnowballOption)
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(
            binary.replace(barrier_state="knocked_in"), market(100, START)
        ),
    )
    assert binary.barrier_state is None
    for value in (True, "0.02"):
        with pytest.raises(TypeError):
            make_fixed(TernarySnowballOption, minimum_coupon_rate=value)
    for value in (0.07, math.inf, math.nan):
        assert_rejection(
            InvalidInputError,
            make_fixed,
            TernarySnowballOption,
            minimum_coupon_rate=value,
        )
    assert_rejection(
        InvalidInputError,
        make_fixed,
        TernarySnowballOption,
        autocall_observations=[],
        knock_in_level=0,
    )
    daily = make_fixed(
        TernarySnowballOption,
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
    )
    assert_rejection(
        MissingHistoryError,
        engine.price,
        daily,
        market(100, datetime(2025, 1, 2)),
    )
    assert (
        engine.price(
            daily.replace(
                barrier_state="knocked_in",
            ),
            market(100, END),
        )
        == 1 + 0.02 * 90 / 365
    )
    # Calendar applicability is checked before history and expiry settlement.
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[0.99, (0.99) * 0.5],
    )
    assert_rejection(
        UnsupportedOperationError,
        engine.price,
        binary,
        market(rate=curve),
    )


def test_preset_rejections_and_strict_numeric_conversion():
    common = {
        "coupon_rate": 0.08,
        "initial_spot": 100,
        "knock_in_level": 80,
        "knock_out_level": 110,
        "autocall_times": [END],
        "effective_time": START,
        "expiry_time": END,
    }
    for field in (
        "coupon_rate",
        "initial_spot",
        "knock_in_level",
        "knock_out_level",
        "principal_redemption_ratio",
    ):
        for value in (True, "100"):
            with pytest.raises(TypeError):
                instruments.standard_snowball(
                    knock_in_observation_clock_time=clock_time(),
                    **(common | {field: value}),
                )
    assert_rejection(
        InvalidInputError,
        instruments.standard_snowball,
        knock_in_observation_clock_time=clock_time(),
        **(common | {"autocall_times": []}),
    )
    assert_rejection(
        InvalidInputError,
        instruments.loss_capped_snowball,
        knock_in_observation_clock_time=clock_time(),
        **common,
        lower_strike=101,
    )
    assert_rejection(
        InvalidInputError,
        instruments.parachute_snowball,
        knock_in_observation_clock_time=clock_time(),
        **common,
        final_knock_out_level=0,
    )
    assert_rejection(
        InvalidInputError,
        instruments.otm_snowball,
        knock_in_observation_clock_time=clock_time(),
        **common,
        upper_strike=math.nan,
    )
    stepped = common.copy()
    stepped["initial_knock_out_level"] = stepped.pop("knock_out_level")
    assert_rejection(
        InvalidInputError,
        instruments.step_down_snowball,
        knock_in_observation_clock_time=clock_time(),
        **stepped,
        knock_out_level_decrement=math.inf,
    )
    # A negative decrement is supported if the expanded levels remain positive.
    note = instruments.step_down_snowball(
        knock_in_observation_clock_time=clock_time(),
        **(stepped | {"autocall_times": [datetime(2025, 2, 3), END]}),
        knock_out_level_decrement=-2,
    )
    assert note.knock_out_levels == ((110, 112))


@pytest.mark.parametrize("kind", (BinarySnowballOption, TernarySnowballOption))
def test_fixed_variants_support_greeks_solving_current_events_and_concurrent_reads(
    kind,
):
    engine = QuadratureAutocallableEngine()
    note = make_fixed(kind)
    ctx = market(103)
    original = repr(note)
    result = numerical_greeks(engine, note, ctx, greeks="all", spot_shift=0.25)
    for name in (
        "delta",
        "gamma",
        "speed",
        "vega",
        "vanna",
        "zomma",
        "rho",
        "theta",
        "charm",
        "color",
    ):
        assert math.isfinite(getattr(result, name)), name
    sigma = implied_volatility(
        engine,
        note,
        ctx,
        result.price,
        lower_bound=0.2,
        upper_bound=0.4,
    )
    assert sigma == pytest.approx(0.3, rel=0, abs=1e-6)
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        engine,
        note,
        ctx,
        10,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert repr(note) == original
    assert ctx.model_parameters.volatility == 0.3
    with ThreadPoolExecutor(max_workers=3) as pool:
        prices = list(
            pool.map(
                lambda s, note=note: engine.price(note, market(s)),
                [80, 100, 120],
            )
        )
    assert prices == ([engine.price(note, market(s)) for s in [80, 100, 120]])
    middle = datetime(2025, 2, 3)
    note = make_fixed(
        kind,
        barrier_state="no_event",
        autocall_observations=autocallable_observations(
            [middle, END], [110, 105], knock_out_coupon_rates=[0.08, 0.06]
        ),
    )
    current = engine.price_with_greeks(
        note,
        market(110, middle),
        greeks="all",
    )
    assert_decimal_equal(current.price, 1 + 0.08 * 33 / 365)
    assert current.delta is None
    assert current.theta is None
    assert ((current.vega, current.rho)) == ((0, 0))
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        note,
        market(110, middle),
        current.price,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    intraday = datetime(2025, 2, 3, 0, 0, 0, 1)
    assert_rejection(
        MissingHistoryError,
        engine.price,
        note.replace(barrier_state=None),
        market(110, intraday),
    )
    assert (
        engine.price(
            note.replace(
                barrier_state="knocked_out",
            ),
            market(100, END),
        )
        == 0
    )
    fixed = make_fixed(
        kind,
        maturity_coupon_rate=0,
        autocall_observations=autocallable_observations(
            [END], [110], knock_out_coupon_rates=[0]
        ),
        **({"minimum_coupon_rate": 0} if kind is TernarySnowballOption else {}),
    )
    assert engine.price(fixed, ctx) == pytest.approx(
        math.exp(-0.04 * 90 / 365), rel=0, abs=1e-15
    )
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        fixed,
        ctx,
        engine.price(fixed, ctx),
        lower_bound=0.0,
        upper_bound=4.0,
    )


@pytest.mark.parametrize(
    "kind,mode",
    (
        (BinarySnowballOption, None),
        (TernarySnowballOption, "at_expiry"),
        (TernarySnowballOption, "every_trading_day"),
    ),
)
def test_multi_event_convergence_for_fixed_variants_and_both_monitoring_modes(
    kind, mode
):
    dates = [datetime(2025, 2, 3), datetime(2025, 3, 3), END]
    ctx = market()
    note = make_fixed(
        kind,
        autocall_observations=autocallable_observations(
            dates, [110, 108, 105], knock_out_coupon_rates=[0.08] * 3
        ),
        **(
            {
                "knock_in_observation": (
                    KnockInObservation.every_trading_day(clock_time())
                    if mode == "every_trading_day"
                    else KnockInObservation.at_expiry()
                ),
                "knock_in_level": 95,
            }
            if mode
            else {}
        ),
    )
    prices = [
        QuadratureAutocallableEngine(
            asset_step_count=n, integration_standard_deviations=e
        ).price(note, ctx)
        for n, e in ((512, 8), (1024, 8), (1024, 10))
    ]
    assert prices[0] == pytest.approx(prices[1], rel=0, abs=1e-4)
    assert prices[1] == pytest.approx(prices[2], rel=0, abs=1e-4)


def test_signed_coupon_and_one_second_digital_regressions():
    end = datetime(2026, 1, 1)
    engine = QuadratureAutocallableEngine()
    for coupon in (0.1, -4):
        for mode in ("at_expiry", "every_trading_day"):
            for kind in (BinarySnowballOption, TernarySnowballOption):
                note = make_fixed(
                    kind,
                    expiry_time=end,
                    maturity_coupon_rate=0,
                    autocall_observations=autocallable_observations(
                        [end], [100], knock_out_coupon_rates=[coupon]
                    ),
                    **(
                        {
                            "minimum_coupon_rate": 0,
                            "knock_in_level": 100,
                            "knock_in_observation": (
                                KnockInObservation.every_trading_day(clock_time())
                                if mode == "every_trading_day"
                                else KnockInObservation.at_expiry()
                            ),
                        }
                        if kind is TernarySnowballOption
                        else {}
                    ),
                )
                d2 = (0.03 - 0.8**2 / 2) / 0.8
                expected = math.exp(-0.04) * (
                    1 + coupon * math.erfc(-d2 / math.sqrt(2)) / 2
                )
                assert engine.price(
                    note,
                    market(volatility=0.8),
                ) == pytest.approx(expected, rel=0, abs=1e-12)
    valuation = datetime(2026, 1, 1) - timedelta(seconds=1)
    t = 1 / (365 * 86400)
    note = make_fixed(
        BinarySnowballOption,
        expiry_time=end,
        maturity_coupon_rate=0,
        autocall_observations=autocallable_observations(
            [end], [100], knock_out_coupon_rates=[0.1]
        ),
    )
    for sigma in (0.01, 0.2):
        for spot in (100, 100 + 50 * sigma * math.sqrt(t)):
            d2 = (math.log(spot / 100) + (0.03 - sigma**2 / 2) * t) / (
                sigma * math.sqrt(t)
            )
            expected = math.exp(-0.04 * t) * (
                1 + 0.1 * math.erfc(-d2 / math.sqrt(2)) / 2
            )
            assert engine.price(
                note,
                market(spot, valuation, volatility=sigma),
            ) == pytest.approx(expected, rel=0, abs=1e-9)
