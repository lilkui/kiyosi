"""Assertions for piecewise affine terminal payoffs and engine coverage."""

from typing import get_args

from kiyosi import Greek


def assert_terminal_greeks(test, result, delta):
    for greek in get_args(Greek):
        if (
            greek not in result.requested
            or greek not in result.supported
            or greek in ("theta", "charm", "color")
            or (
                delta is None and greek in ("delta", "gamma", "speed", "vanna", "zomma")
            )
        ):
            test.assertIsNone(getattr(result, greek), greek)
        else:
            expected = delta if greek == "delta" else 0
            test.assertAlmostEqual(
                result.require(greek), expected, delta=1e-8, msg=greek
            )
