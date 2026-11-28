"""Assertions for piecewise affine terminal payoffs and engine coverage."""

from typing import get_args

import pytest
from kiyosi.pricing import Greek

ALL_GREEKS = get_args(Greek)


def assert_terminal_greeks(
    result, delta, *, requested=ALL_GREEKS, supported=ALL_GREEKS
):
    assert set(result.requested) == set(requested)
    assert set(result.supported) == set(supported)
    for greek in ALL_GREEKS:
        if (
            greek not in requested
            or greek not in supported
            or greek in ("theta", "charm", "color")
            or (
                delta is None
                and greek
                in (
                    "delta",
                    "gamma",
                    "speed",
                    "vanna",
                    "zomma",
                )
            )
        ):
            assert getattr(result, greek) is None, greek
        else:
            expected = delta if greek == "delta" else 0
            assert result.require(greek) == pytest.approx(expected, rel=0, abs=1e-8), (
                greek
            )
