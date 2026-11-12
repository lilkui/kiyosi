"""Regenerate the cached Heston American reference (several minutes).

Notes
-----
QuantLib uses Hundsdorfer-Verwer time stepping and its American payoff
projection, independently of Kiyosi's modified Craig-Sneyd scheme and
Ikonen-Toivanen splitting. First-order extrapolation removes the leading
exercise-time discretization error; consecutive extrapolates estimate uncertainty.

Examples
--------
Run from ``python/tests``::

    python -m quantlib_reference.heston_fd
"""

import json
import math
from itertools import pairwise

import QuantLib as ql


def reference():
    if ql.__version__ != "1.43":
        raise RuntimeError("reference requires QuantLib 1.43")
    with ql.SavedSettings():
        start, end = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
        ql.Settings.instance().evaluationDate = start
        risk = ql.YieldTermStructureHandle(
            ql.FlatForward(start, 0.05, ql.Actual365Fixed())
        )
        dividend = ql.YieldTermStructureHandle(
            ql.FlatForward(start, 0.02, ql.Actual365Fixed())
        )
        model = ql.HestonModel(
            ql.HestonProcess(
                risk,
                dividend,
                ql.QuoteHandle(ql.SimpleQuote(100)),
                0.04,
                1.5,
                0.04,
                0.3,
                -0.7,
            )
        )
        option = ql.VanillaOption(
            ql.PlainVanillaPayoff(ql.Option.Put, 100), ql.AmericanExercise(start, end)
        )
        grids = [400, 800, 1600]
        prices = []
        for n in grids:
            option.setPricingEngine(
                ql.FdHestonVanillaEngine(
                    model,
                    n,
                    n,
                    n // 2,
                    0,
                    ql.FdmSchemeDesc.Hundsdorfer(),
                )
            )
            prices.append(option.NPV())
        extrapolates = [2 * b - a for a, b in pairwise(prices)]
        uncertainty = abs(extrapolates[-1] - extrapolates[-2])
        if not all(map(math.isfinite, prices + extrapolates)):
            raise RuntimeError("non-finite reference")
        if uncertainty > (0.001 + 1e-4 * abs(extrapolates[-1])) / 10:
            raise RuntimeError("reference refinement exceeds uncertainty budget")
        return {
            "quantlib_version": ql.__version__,
            "scheme": "Hundsdorfer-Verwer with American payoff projection",
            "inputs": {
                "valuation": "2025-01-01",
                "expiry": "2026-01-01",
                "spot": 100,
                "strike": 100,
                "option_type": "put",
                "risk_free_rate": 0.05,
                "dividend_yield": 0.02,
                "initial_variance": 0.04,
                "long_run_variance": 0.04,
                "mean_reversion_speed": 1.5,
                "vol_of_vol": 0.3,
                "correlation": -0.7,
            },
            "grids": [{"time": n, "asset": n, "variance": n // 2} for n in grids],
            "raw_prices": prices,
            "extrapolation_order": 1,
            "extrapolated_prices": extrapolates,
            "price": extrapolates[-1],
            "refinement_uncertainty": uncertainty,
        }


if __name__ == "__main__":
    print(json.dumps(reference(), indent=2))
