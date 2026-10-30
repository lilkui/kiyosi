"""Validated American option replacements and ownership."""

import unittest
from datetime import date, datetime

from kiyosi import InvalidInputError
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
        for fields, expected_error in (
            ({"strike": 0}, InvalidInputError),
            ({"expiry_date": date(2024, 1, 1)}, InvalidInputError),
        ):
            with self.assertRaises(expected_error) as error:
                original.replace(**fields)
            self.assertIs(type(error.exception), expected_error)
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
