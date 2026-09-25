#pragma once

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <utility>
#include <vector>

#include <kiyosi/market/calendar.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

namespace detail {

[[nodiscard]] inline Result<Date> following_date(Date target, Date end, const TradingCalendar& calendar)
{
    auto adjusted = target;
    while (adjusted <= end && !calendar.is_trading_day(adjusted))
        adjusted += std::chrono::days{1};
    if (adjusted <= end) return adjusted;
    return std::unexpected(Error{ErrorCategory::invalid_schedule, "schedule Date adjusts past expiry_date"});
}

[[nodiscard]] inline Date add_months(Date value, int months)
{
    const std::chrono::year_month_day source{value};
    const auto target_month = source.year() / source.month() + std::chrono::months{months};
    const auto last_day = std::chrono::year_month_day_last{target_month.year(),
                                                           std::chrono::month_day_last{target_month.month()}}
                              .day();
    return Date{target_month.year() / target_month.month() / std::min(source.day(), last_day)};
}

} // namespace detail

/// Builds candidates at `start + n * interval` for positive `n`; `start` is excluded and `end`
/// is an inclusive upper bound. Each candidate moves forward to the next trading day, duplicate
/// adjusted dates are removed, and generation stops rather than crossing `end`. Consequently,
/// `end` is not guaranteed to be an observation Date. For example, the weekdays calendar maps a
/// daily schedule from 2025-01-03 through 2025-01-07 to [2025-01-06, 2025-01-07]. Supply explicit
/// observation dates to an instrument factory when the contract requires a bespoke terminal Date.
/// @return The validated schedule, or `invalid_date`, `invalid_time_range`, or `invalid_schedule`.
[[nodiscard]] inline Result<ObservationSchedule> make_fixed_interval_schedule(
    Date start, Date end, std::chrono::days interval, const TradingCalendar& calendar = weekdays_calendar())
{
    auto life = validate_instrument_life(start, end);
    if (!life) return std::unexpected(life.error());
    if (interval <= std::chrono::days{0})
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "fixed schedule terms are invalid"});
    std::vector<Date> dates;
    for (auto target = start + interval; target <= end; target += interval) {
        auto adjusted = detail::following_date(target, end, calendar);
        if (!adjusted) break;
        if (dates.empty() || dates.back() != *adjusted) dates.push_back(*adjusted);
    }
    return detail::make_observation_schedule(std::move(dates), start, end, calendar);
}

/// Builds monthly candidates beginning at `start + lock_up_months`; `start` is excluded and `end`
/// is an inclusive upper bound. The start day is clamped to each target month's last day, then each
/// candidate moves forward to the next trading day. Generation stops rather than crossing `end`,
/// so `end` is not guaranteed to be an observation Date. For example, the weekdays calendar maps
/// 2025-01-01 through 2025-03-01 with one lock-up month to [2025-02-03]; the Saturday end candidate
/// would adjust past the bound. Supply explicit observation dates to an instrument factory when
/// the contract requires a bespoke terminal Date.
/// @return The validated schedule, or `invalid_date`, `invalid_time_range`, or `invalid_schedule`.
[[nodiscard]] inline Result<ObservationSchedule> make_monthly_schedule(
    Date start, Date end, int lock_up_months, const TradingCalendar& calendar = weekdays_calendar())
{
    auto life = validate_instrument_life(start, end);
    if (!life) return std::unexpected(life.error());
    if (lock_up_months <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "monthly schedule terms are invalid"});
    const auto start_month = std::chrono::year_month_day{start};
    const auto end_month = std::chrono::year_month_day{end};
    const auto months_until_end =
        (std::int64_t{int(end_month.year())} - int(start_month.year())) * 12 +
        int(unsigned(end_month.month())) - int(unsigned(start_month.month()));
    std::vector<Date> dates;
    for (std::int64_t month = lock_up_months; month <= months_until_end; ++month) {
        const auto target = detail::add_months(start, static_cast<int>(month));
        if (target > end) break;
        auto adjusted = detail::following_date(target, end, calendar);
        if (!adjusted) break;
        dates.push_back(*adjusted);
    }
    return detail::make_observation_schedule(std::move(dates), start, end, calendar);
}

} // namespace kiyosi
