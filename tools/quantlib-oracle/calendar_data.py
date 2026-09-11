"""Generate the SSE weekday holidays from the pinned QuantLib calendar."""

import argparse
from pathlib import Path

import QuantLib as ql


def header():
    if ql.__version__ != "1.43":
        raise RuntimeError("run with the frozen QuantLib environment")
    calendar = ql.China(ql.China.SSE)
    holidays = calendar.holidayList(ql.Date(1, 1, 1901), ql.Date(30, 12, 2199), False)
    last = ql.Date(31, 12, 2199)
    if not calendar.isWeekend(last.weekday()) and calendar.isHoliday(last):
        holidays = (*holidays, last)
    values = [
        str(day.year() * 10000 + int(day.month()) * 100 + day.dayOfMonth())
        for day in holidays
    ]
    rows = [
        "    " + ", ".join(values[offset : offset + 12]) + ","
        for offset in range(0, len(values), 12)
    ]
    return (
        "#pragma once\n\n#include <array>\n\nnamespace kiyosi::detail {\n\ninline constexpr std::array sse_holidays{\n"
        + "\n".join(rows)
        + "\n};\n\n}\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    target = (
        Path(__file__).resolve().parents[2]
        / "include/kiyosi/market/detail/sse_holidays.hpp"
    )
    generated = header()
    if arguments.check:
        assert target.read_text(encoding="utf-8") == generated, (
            "SSE calendar differs from QuantLib"
        )
    else:
        print(generated, end="")
