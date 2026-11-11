"""Validated American option replacements and ownership."""

from datetime import datetime

import pytest
from kiyosi import InvalidInputError
from kiyosi.instruments import AmericanOption, EuropeanOption


@pytest.mark.parametrize("option_class", [AmericanOption, EuropeanOption])
def test_replacements_validate_and_preserve_originals(option_class):
    terms = {
        "option_type": "put",
        "strike": 100,
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
    }
    original = option_class(**terms)
    assert original.replace() == original
    updated = original.replace(
        option_type="call",
        strike=105,
        effective_time=datetime(2026, 1, 1),
        expiry_time=datetime(2027, 1, 1),
    )
    assert updated.option_type == "call"
    assert updated.strike == 105
    assert updated.effective_time == datetime(2026, 1, 1)
    assert updated.expiry_time == datetime(2027, 1, 1)
    for fields, expected_error in (
        ({"strike": 0}, InvalidInputError),
        ({"expiry_time": datetime(2024, 1, 1)}, InvalidInputError),
    ):
        with pytest.raises(expected_error) as error:
            original.replace(**fields)
        assert type(error.value) is expected_error
    for fields in (
        {"strike": True},
        {"strike": None},
        {"option_type": 1},
        {"expiry_time": None},
        {"effective_time": datetime(2025, 1, 1).date()},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            original.replace(**fields)
    assert original == option_class(**terms)
    del original
    assert updated.strike == 105
