#pragma once

#include <chrono>
#include <functional>
#include <utility>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi {

class TradingCalendar;

namespace detail {
// Unchecked construction shared by the venue calendars under kiyosi/market/calendars.
[[nodiscard]] TradingCalendar make_calendar(std::function<bool(Date)>, int);
}

/// Calendar queries follow the library thread-safety default. Predicates supplied to
/// make_trading_calendar must return the same answer for a Date throughout each operation; callers
/// must not depend on query frequency or order. The caller must also ensure that the predicate and
/// any state shared by its copies support concurrent invocation.
class TradingCalendar {
public:
    using TradingDayPredicate = std::function<bool(Date)>;

    [[nodiscard]] bool is_trading_day(Date value) const
    {
        return is_supported_date(value) && predicate_ && predicate_(value);
    }

    int trading_days_per_year() const noexcept { return trading_days_per_year_; }

    [[nodiscard]] Result<int> trading_days_between(Date start, Date end) const
    {
        if (!is_supported_date(start) || !is_supported_date(end))
            return std::unexpected(Error{ErrorCategory::invalid_date,
                                         "calendar range dates must be valid"});
        if (end < start)
            return std::unexpected(Error{ErrorCategory::invalid_time_range,
                                         "calendar range end must not precede start"});
        int count = 0;
        for (auto value = start; value < end; value += std::chrono::days{1})
            count += is_trading_day(value) ? 1 : 0;
        return count;
    }

    [[nodiscard]] Result<double> trading_year_fraction(Date start, Date end) const
    {
        const auto days = trading_days_between(start, end);
        if (!days) return std::unexpected(days.error());
        return static_cast<double>(*days) /
               static_cast<double>(trading_days_per_year_);
    }

private:
    TradingCalendar(TradingDayPredicate predicate, int trading_days_per_year)
        : predicate_(std::move(predicate)), trading_days_per_year_(trading_days_per_year) {}

    friend TradingCalendar detail::make_calendar(TradingDayPredicate, int);

    TradingDayPredicate predicate_;
    int trading_days_per_year_;
};

[[nodiscard]] inline TradingCalendar detail::make_calendar(
    TradingCalendar::TradingDayPredicate predicate, int trading_days_per_year)
{
    return TradingCalendar{std::move(predicate), trading_days_per_year};
}

[[nodiscard]] inline Result<TradingCalendar> make_trading_calendar(
    TradingCalendar::TradingDayPredicate predicate, int trading_days_per_year)
{
    if (!predicate) {
        return std::unexpected(Error{ErrorCategory::invalid_calendar,
                                     "trading calendar requires a day predicate"});
    }
    if (trading_days_per_year <= 0) {
        return std::unexpected(Error{ErrorCategory::invalid_calendar,
                                     "annual trading-day count must be positive"});
    }
    return detail::make_calendar(std::move(predicate), trading_days_per_year);
}

[[nodiscard]] inline TradingCalendar all_days_calendar()
{
    return detail::make_calendar([](Date) { return true; }, 365);
}

/// Holiday-unaware Monday-through-Friday calendar with a 252-day annualization basis.
[[nodiscard]] inline TradingCalendar weekdays_calendar()
{
    return detail::make_calendar(
        [](Date value) {
            const auto weekday = std::chrono::weekday{value};
            return weekday != std::chrono::Saturday && weekday != std::chrono::Sunday;
        },
        252);
}

} // namespace kiyosi
