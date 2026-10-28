"""Candidate-only validated American owning replacements."""

import unittest
from datetime import date, datetime

from kiyosi import ErrorCategory, KiyosiError
from kiyosi.instruments import AmericanOption


class AmericanReplacementTests(unittest.TestCase):
    def test_replacements_validate_and_preserve_originals(self):
        terms = {
            "option_type": "put",
            "strike": 100,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        original = AmericanOption(**terms)
        self.assertEqual(original.replace(), original)
        updated = original.replace(
            option_type="call",
            strike=105,
            effective_date=date(2026, 1, 1),
            expiry_date=date(2027, 1, 1),
        )
        self.assertEqual(updated.option_type, "call")
        self.assertEqual(updated.strike, 105)
        self.assertEqual(updated.effective_date, date(2026, 1, 1))
        self.assertEqual(updated.expiry_date, date(2027, 1, 1))
        for fields, category in (
            ({"strike": 0}, ErrorCategory.INVALID_STRIKE),
            ({"expiry_date": date(2024, 1, 1)}, ErrorCategory.INVALID_TIME_RANGE),
        ):
            with self.assertRaises(KiyosiError) as error:
                original.replace(**fields)
            self.assertEqual(error.exception.category, category)
        for fields in (
            {"strike": True},
            {"strike": None},
            {"option_type": 1},
            {"expiry_date": None},
            {"effective_date": datetime(2025, 1, 1)},
            {"unknown": 1},
        ):
            with self.assertRaises(TypeError):
                original.replace(**fields)
        self.assertEqual(original, AmericanOption(**terms))
        del original
        self.assertEqual(updated.strike, 105)
