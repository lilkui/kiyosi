"""Shared numerical and domain-error assertions."""

import pytest


def assert_rejection(expected_error, operation, *args, **kwargs):
    with pytest.raises(expected_error) as error:
        operation(*args, **kwargs)
    assert type(error.value) is expected_error


def assert_decimal_equal(actual, expected, *, places=7):
    # Decimal-place budgets round the difference rather than setting a relative error.
    assert actual == expected or round(abs(actual - expected), places) == 0
