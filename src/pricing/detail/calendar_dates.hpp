#pragma once

#include <chrono>
#include <vector>

#include <kiyosi/core/time.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi::detail {

/// Trading days in (start, end]; `include_start` also yields `start` when it is a midnight Date.
inline std::vector<Date> trading_dates(
    const TradingCalendar& calendar, Timestamp start, Date end, bool include_start = false)
{
    std::vector<Date> dates;
    auto first = include_start && start == start_of_day(date_of(start)) ? date_of(start)
                                                                       : date_of(start) + std::chrono::days{1};
    for (auto value = first; value <= end; value += std::chrono::days{1})
        if (calendar.is_trading_day(value)) dates.push_back(value);
    return dates;
}

} // namespace kiyosi::detail
