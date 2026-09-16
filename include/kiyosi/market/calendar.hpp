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
[[nodiscard]] TradingCalendar make_calendar(std::function<bool(date)>, int);
}

/// Calendar queries follow the library thread-safety default. For calendars created with
/// make_trading_calendar, the caller must also ensure that the supplied predicate and any state
/// shared by its copies support concurrent invocation.
class TradingCalendar {
public:
    using trading_day_predicate = std::function<bool(date)>;

    [[nodiscard]] bool is_trading_day(date value) const
    {
        return is_valid_date(value) && predicate_ && predicate_(value);
    }

    int annual_trading_days() const noexcept { return annual_trading_days_; }

    [[nodiscard]] int trading_days_between(date start, date end) const
    {
        if (!is_valid_date(start) || !is_valid_date(end) || end < start) return 0;
        int count = 0;
        for (auto value = start; value < end; value += std::chrono::days{1})
            count += is_trading_day(value) ? 1 : 0;
        return count;
    }

    [[nodiscard]] double trading_year_fraction(date start, date end) const
    {
        return static_cast<double>(trading_days_between(start, end)) /
               static_cast<double>(annual_trading_days_);
    }

private:
    TradingCalendar(trading_day_predicate predicate, int annual_trading_days)
        : predicate_(std::move(predicate)), annual_trading_days_(annual_trading_days) {}

    friend TradingCalendar detail::make_calendar(trading_day_predicate, int);

    trading_day_predicate predicate_;
    int annual_trading_days_;
};

[[nodiscard]] inline TradingCalendar detail::make_calendar(
    TradingCalendar::trading_day_predicate predicate, int annual_trading_days)
{
    return TradingCalendar{std::move(predicate), annual_trading_days};
}

[[nodiscard]] inline result<TradingCalendar> make_trading_calendar(
    TradingCalendar::trading_day_predicate predicate, int annual_trading_days)
{
    if (!predicate) {
        return std::unexpected(Error{error_category::invalid_calendar,
                                     "trading calendar requires a day predicate"});
    }
    if (annual_trading_days <= 0) {
        return std::unexpected(Error{error_category::invalid_calendar,
                                     "annual trading-day count must be positive"});
    }
    return detail::make_calendar(std::move(predicate), annual_trading_days);
}

[[nodiscard]] inline TradingCalendar all_days_calendar()
{
    return detail::make_calendar([](date) { return true; }, 365);
}

/// Holiday-unaware Monday-through-Friday calendar with a 252-day annualization basis.
[[nodiscard]] inline TradingCalendar weekdays_calendar()
{
    return detail::make_calendar(
        [](date value) {
            const auto weekday = std::chrono::weekday{value};
            return weekday != std::chrono::Saturday && weekday != std::chrono::Sunday;
        },
        252);
}

} // namespace kiyosi
