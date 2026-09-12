import unittest
from datetime import date

import kiyosi


class KiyosiPythonTests(unittest.TestCase):
    def test_call_and_put_have_stable_result_keys(self):
        args = (100.0, 100.0, date(2025, 1, 1), date(2026, 1, 1), 0.05, 0.02, 0.2)
        call = kiyosi.black_scholes("call", *args)
        put = kiyosi.black_scholes("put", *args)
        self.assertEqual(set(call), {"price", "delta", "gamma", "theta", "vega", "rho"})
        self.assertGreater(call["price"], put["price"])

    def test_expiry_returns_intrinsic_and_none_greeks(self):
        result = kiyosi.black_scholes(
            "call", 110, 100, date(2025, 1, 1), date(2025, 1, 1), 0.05, 0.02, 0.2
        )
        self.assertEqual(result["price"], 10.0)
        self.assertIsNone(result["delta"])

    def test_validation(self):
        with self.assertRaises(ValueError):
            kiyosi.black_scholes(
                "call", 100, 100, date(2026, 1, 1), date(2025, 1, 1), 0.05, 0.02, 0.2
            )
        with self.assertRaises(ValueError):
            kiyosi.black_scholes(
                "call", 100, 100, date(2025, 1, 1), date(2026, 1, 1), 0.05, 0.02, 0.0
            )


if __name__ == "__main__":
    unittest.main()
