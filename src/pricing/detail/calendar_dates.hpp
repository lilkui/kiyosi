#pragma once

#include <chrono>
#include <vector>

#include <kiyosi/core/time.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi::detail {

/// Trading-day engines settle at expiry and cannot price an unadjusted nominal expiry.
inline Result<void> validate_trading_expiry(const TradingCalendar& calendar, Date expiry_date)
{
    if (!calendar.is_trading_day(expiry_date))
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "expiry_date must be a trading day; adjust the nominal date first"});
    return {};
}

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
