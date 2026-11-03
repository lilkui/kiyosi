"""Validated digital option replacements and ownership."""

import unittest
from datetime import date

from kiyosi import InvalidInputError
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption


class DigitalReplacementTests(unittest.TestCase):
    def test_replacements_validate_and_preserve_originals(self):
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        for cls in (CashOrNothingOption, AssetOrNothingOption):
            extra = {"payout": 10} if cls is CashOrNothingOption else {}
            original = cls(**terms, **extra)
            self.assertEqual(original.replace(), original)
            replaced = original.replace(
                option_type="put", strike=105, expiry_date=date(2027, 1, 1)
            )
            self.assertEqual(replaced.option_type, "put")
            self.assertEqual(replaced.strike, 105)
            self.assertEqual(replaced.expiry_date, date(2027, 1, 1))
            for field, expected_error in (
                ("strike", InvalidInputError),
                ("expiry_date", InvalidInputError),
            ):
                with self.assertRaises(expected_error) as error:
                    original.replace(
                        **{field: 0 if field == "strike" else date(2024, 1, 1)}
                    )
                self.assertIs(type(error.exception), expected_error)
            for fields in (
                {"strike": True},
                {"strike": None},
                {"option_type": 1},
                {"expiry_date": None},
                {"unknown": 1},
            ):
                with self.assertRaises(TypeError):
                    original.replace(**fields)
            self.assertEqual(original, cls(**terms, **extra))
            if extra:
                self.assertEqual(original.replace(payout=7).payout, 7)
                with self.assertRaises(InvalidInputError) as error:
                    original.replace(payout=0)
                self.assertIs(type(error.exception), InvalidInputError)
                self.assertEqual(original.payout, 10)
            else:
                with self.assertRaises(TypeError):
                    original.replace(payout=7)
            del original
            self.assertEqual(replaced.strike, 105)
