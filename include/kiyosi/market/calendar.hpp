#pragma once

#include <span>
#include <functional>
#include <algorithm>
#include <array>
#include <chrono>
#include <utility>
#include <vector>
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
                                      (year < std::chrono::year{1991} || year > std::chrono::year{2030} ||
                                       !std::ranges::binary_search(detail::sse_holidays, encoded));
                           },
                           243};
}

[[nodiscard]] inline TradingCalendar shanghai_stock_exchange_calendar() { return sse_calendar(); }
[[nodiscard]] inline TradingCalendar shanghai_calendar() { return sse_calendar(); }
[[nodiscard]] inline TradingCalendar sse_exchange_calendar() { return sse_calendar(); }

[[nodiscard]] inline result<void> validate_observation_date(
    date observation, date instrument_start, date instrument_end, const TradingCalendar& calendar)
{
    if (!is_valid_date(observation) || !is_valid_date(instrument_start) || !is_valid_date(instrument_end) ||
        instrument_end < instrument_start || observation < instrument_start || instrument_end < observation) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "observation date must be within the instrument life"});
    }
    if (!calendar.is_trading_day(observation)) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "observation date is not a trading day"});
    }
    return {};
}

[[nodiscard]] inline result<void> validate_observation_dates(
    std::span<const date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    if (!is_valid_date(instrument_start) || !is_valid_date(instrument_end) || instrument_end < instrument_start) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "instrument life must be a valid ordered date range"});
    }
    for (std::size_t index = 0; index < observations.size(); ++index) {
        if (index > 0 && observations[index] <= observations[index - 1]) {
            return std::unexpected(Error{error_category::invalid_date,
                                         "observation dates must be strictly ordered"});
        }
        auto valid = validate_observation_date(observations[index], instrument_start, instrument_end, calendar);
        if (!valid) return std::unexpected(valid.error());
    }
    return {};
}

[[nodiscard]] inline result<void> validate_schedule(
    std::span<const date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, instrument_start, instrument_end, calendar);
}

class ObservationSchedule {
public:
    const std::vector<date>& dates() const noexcept { return dates_; }
    std::size_t size() const noexcept { return dates_.size(); }
    bool empty() const noexcept { return dates_.empty(); }
    const date& operator[](std::size_t index) const noexcept { return dates_[index]; }
    auto begin() const noexcept { return dates_.begin(); }
    auto end() const noexcept { return dates_.end(); }

    friend bool operator==(const ObservationSchedule&, const ObservationSchedule&) = default;

private:
    explicit ObservationSchedule(std::vector<date> dates) : dates_(std::move(dates)) {}
    std::vector<date> dates_;

    friend result<ObservationSchedule> make_observation_schedule(
        std::vector<date>, date, date, const TradingCalendar&);
};

[[nodiscard]] inline result<void> validate_schedule(
    const ObservationSchedule& schedule, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(schedule.dates(), instrument_start, instrument_end, calendar);
}

[[nodiscard]] inline result<ObservationSchedule> make_observation_schedule(
    std::vector<date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    auto valid = validate_observation_dates(observations, instrument_start, instrument_end, calendar);
    if (!valid) return std::unexpected(valid.error());
    return ObservationSchedule{std::move(observations)};
}

enum class schedule_adjustment : unsigned char { following };

namespace detail {
[[nodiscard]] inline result<date> following_date(date target, date end, const TradingCalendar& calendar)
{
    auto adjusted = target;
    while (adjusted <= end && !calendar.is_trading_day(adjusted)) adjusted += std::chrono::days{1};
    if (adjusted <= end) return adjusted;
    return std::unexpected(Error{error_category::invalid_schedule, "schedule date adjusts past expiry"});
}

[[nodiscard]] inline date add_months(date value, int months)
{
    const std::chrono::year_month_day source{value};
    const auto target_month = source.year() / source.month() + std::chrono::months{months};
    const auto last_day = std::chrono::year_month_day_last{target_month.year(),
                                                            std::chrono::month_day_last{target_month.month()}}.day();
    return date{target_month.year() / target_month.month() / std::min(source.day(), last_day)};
}
}

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
    return make_observation_schedule(std::move(dates), start, end, calendar);
}

[[nodiscard]] inline result<ObservationSchedule> make_fixed_interval_schedule(
    date start, date end, int interval_days, const TradingCalendar& calendar = exchange_calendar())
{
    return make_fixed_interval_schedule(start, end, std::chrono::days{interval_days}, calendar);
}

[[nodiscard]] inline result<ObservationSchedule> make_fixed_schedule(
    date start, date end, std::chrono::days interval, const TradingCalendar& calendar = exchange_calendar())
{
    return make_fixed_interval_schedule(start, end, interval, calendar);
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
    return make_observation_schedule(std::move(dates), start, end, calendar);
}

[[nodiscard]] inline result<ObservationSchedule> make_monthly_observation_schedule(
    date start, date end, int lock_up_months, const TradingCalendar& calendar = exchange_calendar())
{
    return make_monthly_schedule(start, end, lock_up_months, calendar);
}

} // namespace kiyosi
