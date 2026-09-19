"""Generate the SSE weekday holidays from the pinned QuantLib calendar."""

import argparse
import re
from pathlib import Path

import QuantLib as ql


def holidays():
    if ql.__version__ != "1.43":
        raise RuntimeError("run with the frozen QuantLib environment")
    calendar = ql.China(ql.China.SSE)
    holidays = calendar.holidayList(ql.Date(1, 1, 1901), ql.Date(30, 12, 2199), False)
    last = ql.Date(31, 12, 2199)
    if not calendar.isWeekend(last.weekday()) and calendar.isHoliday(last):
        holidays = (*holidays, last)
    return [
        day.year() * 10000 + int(day.month()) * 100 + day.dayOfMonth()
        for day in holidays
    ]


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    target = Path(__file__).resolve().parents[1] / "src/market/calendars/sse.cpp"
    generated = holidays()
    if arguments.check:
        match = re.search(
            r"constexpr std::array sse_holidays\{([\d\s,]+)\};",
            target.read_text(encoding="utf-8"),
        )
        if (
            match is None
            or [int(value) for value in match[1].split(",") if value.strip()]
            != generated
        ):
            raise SystemExit("SSE calendar differs from QuantLib")
        print("SSE calendar matches QuantLib")
    else:
        print("constexpr std::array sse_holidays{")
        for offset in range(0, len(generated), 12):
            print("    " + ", ".join(map(str, generated[offset : offset + 12])) + ",")
        print("};")
