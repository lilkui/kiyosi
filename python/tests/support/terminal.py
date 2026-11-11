"""Assertions for piecewise affine terminal payoffs and engine coverage."""

from typing import get_args

import pytest
from kiyosi import Greek


def assert_terminal_greeks(result, delta):
    for greek in get_args(Greek):
        if (
            greek not in result.requested
            or greek not in result.supported
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
