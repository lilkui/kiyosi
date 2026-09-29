# SSE trading calendar data

The [generator](../tools/generate_sse_calendar.py) fetches the [XSHG source file](https://github.com/gerrymanoim/exchange_calendars/blob/HEAD/exchange_calendars/exchange_calendar_xshg.py) from the upstream default branch each time it runs. That branch is currently `master`; the repository has no `main` branch. The script reads `precomputed_shanghai_holidays` without importing or executing upstream code. Its latest recorded year is currently 2026, matching the [SSE's published schedule](https://www.sse.com.cn/disclosure/dealinstruc/closed/).

For years after the upstream list through 2099, the generator gets lunar festival and Qingming dates from `lunar-python` and applies a simple estimate based on the [current holiday policy](https://www.gov.cn/gongbao/2024/issue_11726/material/gwygb202433.pdf): eight consecutive days from Lunar New Year's Eve, five around May 1–2, October 1–7, and three around the New Year, Qingming, Dragon Boat, and Mid-Autumn holidays (Wednesday stays one day). When Mid-Autumn falls during October 1–7, the National Day break extends through October 8. It does not predict one-off exchange closures or future State Council adjustments. Re-run the generator as XSHG publishes new years.

Regenerate from the repository root:

```sh
uv run --no-project --with lunar-python==1.4.8 python tools/generate_sse_calendar.py --write
uv run --no-project --with lunar-python==1.4.8 python tools/generate_sse_calendar.py --check
```

The generated C++ table is static; the C++ core needs neither Python nor a calendar package. Outside 1991–2099, the calendar treats weekdays as trading days.
