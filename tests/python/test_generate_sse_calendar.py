"""Checks that the SSE generator uses the live XSHG list."""

import io
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
import generate_sse_calendar as generator  # noqa: E402


class GenerateSseCalendarTests(unittest.TestCase):
    def test_live_list_sets_forecast_boundary(self):
        source = b'precomputed_shanghai_holidays = pd.to_datetime(["1991-01-01", "2027-01-01"])'
        with (
            patch.object(
                generator.urllib.request, "urlopen", return_value=io.BytesIO(source)
            ) as fetch,
            patch.object(generator, "forecast", return_value=set()) as forecast,
        ):
            self.assertEqual(generator.holidays(), [19910101, 20270101])
        fetch.assert_called_once_with(generator.SOURCE, timeout=30)
        self.assertEqual(forecast.call_args_list[0].args, (2028,))


if __name__ == "__main__":
    unittest.main()
