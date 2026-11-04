"""Shared FD model contracts through the public Python interface."""

import unittest
from datetime import timedelta

from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilitySurface,
    RateTermStructure,
)
from test_local_volatility import END, START, context, surface
from test_support_matrix import HESTON_ENGINES, terminal_cases


def representatives():
    found = set()
    for contract, names, _ in terminal_cases():
        for name in names:
            if (
                name.startswith("FiniteDifference")
                and name not in HESTON_ENGINES
                and name not in found
            ):
                found.add(name)
                yield contract, name


class LocalVolatilityFDContractTests(unittest.TestCase):
    def test_every_fd_scheme_preserves_constant_surface_bsm_results(self):
        local = context()
        bsm = local.replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            )
        )
        for contract, name in representatives():
            for scheme in [
                "explicit_euler",
                "implicit_euler",
                "crank_nicolson",
                "rannacher",
            ]:
                with self.subTest(engine=name, scheme=scheme):
                    engine = getattr(pricing, name)(
                        asset_step_count=80,
                        time_step_count=16000,
                        asset_upper_boundary=400,
                        scheme=scheme,
                    )
                    actual, expected = (
                        engine.price(contract, local),
                        engine.price(contract, bsm),
                    )
                    self.assertAlmostEqual(actual, expected, delta=1e-6)

    def test_active_products_reject_missing_surface_coverage_and_unstable_steps(self):
        short = context(
            LocalVolatilitySurface(
                times=[START, START + timedelta(days=2)],
                spots=[80, 120],
                values=[[0.2, 0.2], [0.2, 0.2]],
            )
        )
        for contract, name in representatives():
            with self.subTest(engine=name):
                with self.assertRaises(InvalidInputError):
                    getattr(pricing, name)().price(contract, short)
                with self.assertRaises(InvalidInputError):
                    getattr(pricing, name)(
                        asset_step_count=800, time_step_count=1, scheme="explicit_euler"
                    ).price(contract, context())

    def test_digital_rejects_node_rates_even_at_expiry(self):
        curve = RateTermStructure(
            reference_time=START,
            times=[END, END + (END - START)],
            factors=[0.95, (0.95) * 0.5],
        )
        for contract, name in representatives():
            if name != "FiniteDifferenceDigitalEngine":
                continue
            for field in ["risk_free_rate", "dividend_yield"]:
                for time in [START, END]:
                    local = context(surface()).replace(valuation_time=time)
                    local = local.replace(
                        model_parameters=local.model_parameters.replace(
                            **{field: curve}
                        )
                    )
                    with (
                        self.subTest(field=field, time=time),
                        self.assertRaises(UnsupportedOperationError),
                    ):
                        getattr(pricing, name)().price(contract, local)


if __name__ == "__main__":
    unittest.main()
