#pragma once

#include <algorithm>
#include <chrono>
#include <utility>
#include <vector>

#include <kiyosi/market/calendar.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

enum class schedule_adjustment : unsigned char { following };

namespace detail {

[[nodiscard]] inline result<date> following_date(date target, date end, const TradingCalendar& calendar)
{
    auto adjusted = target;
    while (adjusted <= end && !calendar.is_trading_day(adjusted))
        adjusted += std::chrono::days{1};
    if (adjusted <= end) return adjusted;
    return std::unexpected(Error{error_category::invalid_schedule, "schedule date adjusts past expiry"});
}

[[nodiscard]] inline date add_months(date value, int months)
{
    const std::chrono::year_month_day source{value};
    const auto target_month = source.year() / source.month() + std::chrono::months{months};
    const auto last_day = std::chrono::year_month_day_last{target_month.year(),
                                                           std::chrono::month_day_last{target_month.month()}}
                              .day();
    return date{target_month.year() / target_month.month() / std::min(source.day(), last_day)};
}

} // namespace detail

[[nodiscard]] inline result<ObservationSchedule> make_fixed_interval_schedule(
    date start, date end, std::chrono::days interval, const TradingCalendar& calendar = exchange_calendar())
{
    if (!is_valid_date(start) || !is_valid_date(end) || end < start || interval <= std::chrono::days{0})
        return std::unexpected(Error{error_category::invalid_schedule, "fixed schedule terms are invalid"});
    std::vector<date> dates;
    for (auto target = start + interval; target <= end; target += interval) {
        auto adjusted = detail::following_date(target, end, calendar);
        if (!adjusted) break;
        if (dates.empty() || dates.back() != *adjusted) dates.push_back(*adjusted);
    }
    return detail::make_observation_schedule(std::move(dates), start, end, calendar);
}

[[nodiscard]] inline result<ObservationSchedule> make_monthly_schedule(
    date start, date end, int lock_up_months, const TradingCalendar& calendar = exchange_calendar())
{
    if (!is_valid_date(start) || !is_valid_date(end) || end < start || lock_up_months <= 0)
        return std::unexpected(Error{error_category::invalid_schedule, "monthly schedule terms are invalid"});
    std::vector<date> dates;
    for (int month = lock_up_months;; ++month) {
        const auto target = detail::add_months(start, month);
        if (target > end) break;
        auto adjusted = detail::following_date(target, end, calendar);
        if (!adjusted) break;
        dates.push_back(*adjusted);
    }
    return detail::make_observation_schedule(std::move(dates), start, end, calendar);
}

} // namespace kiyosi
