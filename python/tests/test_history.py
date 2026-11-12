"""Valuation-relative history validation through public interfaces."""

from datetime import datetime, time, timedelta

import pytest
from kiyosi import InvalidInputError, MissingHistoryError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from kiyosi.instruments import KnockInObservation

AT = datetime(2025, 1, 6, 12)


def context(at=AT, volatility=0.2):
    return m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=volatility
        ),
        spot_price=100,
        valuation_time=at,
    )


def test_autocallable_history_respects_first_daily_observation_on_sparse_calendar(
    subtests,
):
    start, first, end = (
        datetime(2025, 1, 2, 13),
        datetime(2025, 1, 6, 12),
        datetime(2025, 1, 6, 16),
    )
    note = i.SnowballOption(
        maturity_coupon_rate=0,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        effective_time=start,
        expiry_time=end,
        autocall_observations=[
            i.AutocallableObservation.snowball(end, 110, coupon_rate=0)
        ],
        knock_in_observation=KnockInObservation.every_trading_day(time(12)),
        calendar=m.custom_calendar(
            base=m.all_days_calendar(),
            closed_dates=[(start + timedelta(days=day)).date() for day in (1, 2, 3)],
        ),
    )
    selected = p.QuadratureAutocallableEngine()
    for at in (
        start,
        first - timedelta(microseconds=1),
        first,
        first + timedelta(microseconds=1),
    ):
        snapshot = context(at, volatility=0)
        with subtests.test(at=at):
            if at <= first:
                assert selected.price(note, snapshot) == 1
            else:
                with pytest.raises(MissingHistoryError):
                    selected.price(note, snapshot)
            knocked_in = note.replace(barrier_state="knocked_in")
            if at <= first:
                with pytest.raises(InvalidInputError):
                    selected.price(knocked_in, snapshot)
            else:
                assert selected.price(knocked_in, snapshot) == 1
            known = note.replace(barrier_state="no_event")
            assert selected.price(known, snapshot) == 1
