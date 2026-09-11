#pragma once

#include <functional>
#include <algorithm>
#include <chrono>
#include <utility>
#include <kiyosi/core/types.hpp>
#include <kiyosi/market/detail/sse_holidays.hpp>

namespace kiyosi {

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

    friend result<TradingCalendar> make_trading_calendar(trading_day_predicate, int);
    friend TradingCalendar all_days_calendar();
    friend TradingCalendar exchange_calendar();
    friend TradingCalendar sse_calendar();

    trading_day_predicate predicate_;
    int annual_trading_days_;
};

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
    return TradingCalendar{std::move(predicate), annual_trading_days};
}

[[nodiscard]] inline TradingCalendar all_days_calendar()
{
    return TradingCalendar{[](date) { return true; }, 365};
}

[[nodiscard]] inline TradingCalendar exchange_calendar()
{
    return TradingCalendar{[](date value) {
                               const auto weekday = std::chrono::weekday{value};
                               return weekday != std::chrono::Saturday && weekday != std::chrono::Sunday;
                           },
                           252};
}

[[nodiscard]] inline TradingCalendar sse_calendar()
{
    return TradingCalendar{[](date value) {
                               const auto weekday = std::chrono::weekday{value};
                               const auto parts = std::chrono::year_month_day{value};
                               const auto year = parts.year();
                               const int encoded = int(year) * 10000 + int(unsigned(parts.month())) * 100 + int(unsigned(parts.day()));
                               return weekday != std::chrono::Saturday && weekday != std::chrono::Sunday &&
                                      !std::ranges::binary_search(detail::sse_holidays, encoded);
                           },
                           243};
}

} // namespace kiyosi
