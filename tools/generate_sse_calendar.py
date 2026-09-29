"""Generate SSE weekday closures from XSHG history and a simple holiday forecast.

Run with: uv run --no-project --with lunar-python==1.4.8 python tools/generate_sse_calendar.py --write
"""

import argparse
import ast
import datetime as dt
import re
import urllib.request
from pathlib import Path

LAST_YEAR = 2099
ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "src/market/calendars/sse.cpp"
TABLE = re.compile(
    r"constexpr auto sse_holidays = std::to_array<int>\(\{.*?\}\);", re.S
)
SOURCE = (
    "https://raw.githubusercontent.com/gerrymanoim/exchange_calendars/HEAD/"
    "exchange_calendars/exchange_calendar_xshg.py"
)


def dates(start, count):
    return {start + dt.timedelta(days=offset) for offset in range(count)}


def long_weekend(holiday):
    """Approximate the usual three-day break, or a lone Wednesday closure."""
    weekday = holiday.weekday()
    if weekday == 2:
        return {holiday}
    start = holiday - dt.timedelta(
        days=2 if weekday in (0, 1) else 1 if weekday == 6 else 0
    )
    return dates(start, 3)


def labor_break(year):
    may_day = dt.date(year, 5, 1)
    starts = (may_day + dt.timedelta(days=offset) for offset in range(-3, 1))
    return max(
        (dates(start, 5) for start in starts),
        key=lambda block: (sum(day.weekday() >= 5 for day in block), min(block)),
    )


def forecast(year):
    try:
        from lunar_python import Lunar, Solar
    except ImportError as error:
        raise RuntimeError(
            "install lunar-python==1.4.8 to regenerate the SSE table"
        ) from error

    spring = dt.date.fromisoformat(Lunar.fromYmd(year, 1, 1).getSolar().toYmd())
    dragon = dt.date.fromisoformat(Lunar.fromYmd(year, 5, 5).getSolar().toYmd())
    mid_autumn = dt.date.fromisoformat(Lunar.fromYmd(year, 8, 15).getSolar().toYmd())
    qingming = dt.date.fromisoformat(
        Solar.fromYmd(year, 4, 4).getLunar().getJieQiTable()["清明"].toYmd()
    )

    # ponytail: this predicts the usual contiguous breaks, not future ad-hoc
    # State Council shifts; replace each year with the SSE announcement when published.
    closed = dates(spring - dt.timedelta(days=1), 8)
    closed.update(dates(dt.date(year, 10, 1), 7))
    if dt.date(year, 10, 1) <= mid_autumn <= dt.date(year, 10, 7):
        closed.add(dt.date(year, 10, 8))
    closed.update(labor_break(year))
    for holiday in (dt.date(year, 1, 1), qingming, dragon, mid_autumn):
        closed.update(long_weekend(holiday))
    return closed


def recorded_holidays():
    with urllib.request.urlopen(SOURCE, timeout=30) as response:
        source = response.read().decode("utf-8")
    match = re.search(
        r"precomputed_shanghai_holidays\s*=\s*pd\.to_datetime\(\s*(\[.*?\])\s*\)",
        source,
        re.S,
    )
    if match is None:
        raise ValueError("XSHG holiday list not found in upstream source")
    recorded = [dt.date.fromisoformat(value) for value in ast.literal_eval(match[1])]
    if not recorded or recorded != sorted(set(recorded)) or recorded[0].year != 1991:
        raise ValueError(
            "XSHG holiday list is empty, unsorted, or changed its starting year"
        )
    return recorded


def holidays():
    recorded = recorded_holidays()
    closed = {day for day in recorded if day.year <= LAST_YEAR}
    for year in range(recorded[-1].year + 1, LAST_YEAR + 1):
        closed.update(forecast(year))
    return [int(day.strftime("%Y%m%d")) for day in sorted(closed) if day.weekday() < 5]


def render(values):
    lines = ["constexpr auto sse_holidays = std::to_array<int>({"]
    for offset in range(0, len(values), 12):
        lines.append("    " + ", ".join(map(str, values[offset : offset + 12])) + ",")
    return "\n".join((*lines, "});"))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--check", action="store_true")
    action.add_argument("--write", action="store_true")
    arguments = parser.parse_args()
    values = holidays()
    generated = render(values)
    if arguments.check or arguments.write:
        source = TARGET.read_text(encoding="utf-8")
        match = TABLE.search(source)
        if match is None:
            raise SystemExit("SSE holiday table not found")
        if arguments.check:
            actual = [int(value) for value in re.findall(r"\d{8}", match.group())]
            if actual != values:
                raise SystemExit(
                    "SSE holiday table differs from XSHG history and forecast"
                )
            print("SSE holiday table matches XSHG history and forecast")
        else:
            updated = source[: match.start()] + generated + source[match.end() :]
            TARGET.write_text(updated, encoding="utf-8")
    else:
        print(generated)
