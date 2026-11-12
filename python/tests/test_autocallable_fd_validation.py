"""Autocallable finite-difference engine validation precedence."""

from datetime import datetime

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    pricing,
)
from kiyosi.instruments import AutocallableObservation, BinarySnowballOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    weekdays_calendar,
)


@pytest.mark.parametrize("history", (None, "no_event"))
def test_autocallable_engines_share_domain_validation_precedence(subtests, history):
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 4)
    fields = {
        "maturity_coupon_rate": 0.05,
        "barrier_state": history,
        "effective_time": start,
        "observations": [
            AutocallableObservation.snowball(
                datetime(2025, 1, 2), 110, coupon_rate=0.08
            )
        ],
    }
    with pytest.raises(InvalidInputError):
        BinarySnowballOption(
            calendar=weekdays_calendar(),
            **(
                fields
                | {
                    "observations": [
                        AutocallableObservation.snowball(end, 110, coupon_rate=0)
                    ]
                }
            ),
            expiry_time=end,
        )
    note = BinarySnowballOption(
        calendar=weekdays_calendar(),
        **fields,
        expiry_time=datetime(2025, 1, 3),
    )
    valuations = [datetime(2024, 12, 31), datetime(2025, 1, 4)]
    if history is None:
        valuations.append(datetime(2025, 1, 3))
    for valuation in valuations:
        expected = (
            MissingHistoryError
            if history is None and valuation == note.expiry_time
            else InvalidInputError
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03,
                dividend_yield=0,
                volatility=0.2,
            ),
            spot_price=100,
            valuation_time=valuation,
        )
        for engine in (
            pricing.FiniteDifferenceAutocallableEngine(),
            pricing.MonteCarloAutocallableEngine(path_count=2, seed=7),
        ):
            for operation in (
                lambda engine=engine, note=note, context=context: engine.price(
                    note, context
                ),
                lambda engine=engine, note=note, context=context: (
                    engine.price_with_greeks(note, context, "delta")
                ),
                lambda engine=engine, note=note, context=context: (
                    pricing.implied_volatility(
                        engine,
                        note,
                        context,
                        1,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )
                ),
            ):
                with subtests.test(
                    history=history, valuation=valuation, engine=type(engine)
                ):
                    with pytest.raises(expected) as error:
                        operation()
                    assert type(error.value) is expected
