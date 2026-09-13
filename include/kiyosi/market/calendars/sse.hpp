#pragma once

#include <algorithm>
#include <chrono>

#include <kiyosi/market/calendar.hpp>
#include <kiyosi/market/calendars/detail/sse_holidays.hpp>

namespace kiyosi {

/// Shanghai Stock Exchange: weekdays excluding the published mainland holiday closures.
[[nodiscard]] inline TradingCalendar sse_calendar()
{
    return detail::make_calendar(
        [](date value) {
            const auto weekday = std::chrono::weekday{value};
            if (weekday == std::chrono::Saturday || weekday == std::chrono::Sunday) return false;
            const auto parts = std::chrono::year_month_day{value};
            const int encoded = int(parts.year()) * 10000 + int(unsigned(parts.month())) * 100 +
                                int(unsigned(parts.day()));
            return !std::ranges::binary_search(detail::sse_holidays, encoded);
        },
        243);
}

} // namespace kiyosi
