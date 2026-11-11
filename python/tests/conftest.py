"""Fixtures shared by the Python runtime suite."""

import pytest
import QuantLib as ql

pytest.register_assert_rewrite("support.assertions", "support.terminal")


@pytest.fixture
def quantlib_settings():
    with ql.SavedSettings():
        yield
