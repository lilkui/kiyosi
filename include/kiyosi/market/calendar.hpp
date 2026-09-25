#pragma once

#include <chrono>
#include <functional>
#include <utility>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi {

class TradingCalendar;

/// Direction used to move a nominal contract date onto a trading day.
enum class BusinessDayConvention {
    following, ///< Move to the next trading day, including the nominal date.
    preceding  ///< Move to the previous trading day, including the nominal date.
};

namespace detail {
// Unchecked construction shared by the venue calendars under kiyosi/market/calendars.
[[nodiscard]] TradingCalendar make_calendar(std::function<bool(Date)>, int);
} // namespace detail

/// Calendar queries follow the library thread-safety default. Predicates supplied to
/// make_trading_calendar must return the same answer for a Date throughout each operation; callers
/// must not depend on query frequency or order. The caller must also ensure that the predicate and
/// any state shared by its copies support concurrent invocation.
class TradingCalendar {
public:
    /// Predicate returning whether a supported date is a trading day.
    using TradingDayPredicate = std::function<bool(Date)>;

    /// Tests whether a supported date is a trading day.
    /// @return `false` for unsupported dates or when the predicate rejects the date.
    [[nodiscard]] bool is_trading_day(Date value) const
    {
        return is_supported_date(value) && predicate_ && predicate_(value);
    }

    /// Adjusts a nominal date to a trading day under the chosen convention.
    /// @return The adjusted date, or `invalid_date` if no trading day exists in the
    /// supported direction; an unknown convention returns `invalid_parameter`.
    [[nodiscard]] Result<Date> adjust(Date nominal, BusinessDayConvention convention) const
    {
        if (!is_supported_date(nominal))
            return std::unexpected(Error{ErrorCategory::invalid_date, "nominal date is unsupported"});
        std::chrono::days direction{};
        switch (convention) {
        case BusinessDayConvention::following:
            direction = std::chrono::days{1};
            break;
        case BusinessDayConvention::preceding:
            direction = std::chrono::days{-1};
            break;
        default:
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "business-day convention is invalid"});
        }
        for (auto value = nominal; is_supported_date(value); value += direction)
            if (is_trading_day(value)) return value;
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "no trading day exists in the adjustment direction"});
    }

    /// Returns the positive annual trading-day basis.
    int trading_days_per_year() const noexcept { return trading_days_per_year_; }

    /// Counts trading days in the half-open interval `[start, end)`.
    /// @return The count, or an `invalid_date` or `invalid_time_range` error.
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

    /// Computes a trading-day year fraction over `[start, end)`.
    /// @return Trading days divided by trading_days_per_year(), or a range error.
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
    std::function<bool(Date)> predicate, int trading_days_per_year)
{
    return TradingCalendar{std::move(predicate), trading_days_per_year};
}

/// Creates a validated trading calendar.
/// @param predicate Callable returning whether a date is a trading day.
/// @param trading_days_per_year Positive annualization basis.
/// @return The calendar, or an `invalid_calendar` error.
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

/// Creates a calendar in which every supported date is a trading day.
/// @return Calendar with a 365-day annualization basis.
[[nodiscard]] inline TradingCalendar all_days_calendar()
{
    return detail::make_calendar([](Date) { return true; }, 365);
}

/// Holiday-unaware Monday-through-Friday calendar with a 252-day annualization basis.
/// @return A weekday-only calendar.
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
