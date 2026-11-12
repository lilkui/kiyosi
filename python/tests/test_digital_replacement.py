"""Validated digital option replacements and ownership."""

from datetime import datetime

import pytest
from kiyosi import InvalidInputError
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption


@pytest.mark.parametrize("cls", (CashOrNothingOption, AssetOrNothingOption))
def test_replacements_validate_and_preserve_originals(cls):
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
    }
    extra = {"payout": 10} if cls is CashOrNothingOption else {}
    original = cls(**terms, **extra)
    assert original.replace() == original
    replaced = original.replace(
        option_type="put", strike=105, expiry_time=datetime(2027, 1, 1)
    )
    assert replaced.option_type == "put"
    assert replaced.strike == 105
    assert replaced.expiry_time == datetime(2027, 1, 1)
    for field, expected_error in (
        ("strike", InvalidInputError),
        ("expiry_time", InvalidInputError),
    ):
        with pytest.raises(expected_error) as error:
            original.replace(
                **{field: 0 if field == "strike" else datetime(2024, 1, 1)}
            )
        assert type(error.value) is expected_error
    for fields in (
        {"strike": True},
        {"strike": None},
        {"option_type": 1},
        {"expiry_time": None},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            original.replace(**fields)
    assert original == cls(**terms, **extra)
    if extra:
        assert original.replace(payout=7).payout == 7
        with pytest.raises(InvalidInputError) as error:
            original.replace(payout=0)
        assert type(error.value) is InvalidInputError
        assert original.payout == 10
    else:
        with pytest.raises(TypeError):
            original.replace(payout=7)
    del original
    assert replaced.strike == 105
