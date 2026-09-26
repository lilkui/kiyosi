#pragma once

#include <span>
#include <utility>
#include <vector>

#include <kiyosi/core/time.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi {

class ObservationSchedule;

namespace detail {
[[nodiscard]] Result<ObservationSchedule> make_date_schedule(std::vector<Date>, Date, Date);
[[nodiscard]] Result<ObservationSchedule> make_observation_schedule(
    std::vector<Date>, Date, Date, const TradingCalendar&);
} // namespace detail

/// Validates ordering and instrument-life bounds for a date schedule.
/// @return Success, or `invalid_date`, `invalid_time_range`, or `invalid_schedule`.
[[nodiscard]] inline Result<void> validate_date_schedule(
    std::span<const Date> observation_dates, Date instrument_start, Date instrument_end)
{
    auto life = validate_instrument_life(instrument_start, instrument_end);
    if (!life) return life;
    for (std::size_t index = 0; index < observation_dates.size(); ++index) {
        if (!is_supported_date(observation_dates[index]))
            return std::unexpected(Error{ErrorCategory::invalid_date, "observation date is invalid"});
        if (index > 0 && observation_dates[index] <= observation_dates[index - 1])
            return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                         "observation dates must be strictly ordered"});
        if (observation_dates[index] < instrument_start || observation_dates[index] > instrument_end)
            return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                         "observation Date must be within the instrument life"});
    }
    return {};
}

/// Validates one observation date against an instrument life and calendar.
/// @return Success, or `invalid_date`, `invalid_time_range`, or `invalid_schedule`.
[[nodiscard]] inline Result<void> validate_observation_date(
    Date observation_date, Date instrument_start, Date instrument_end, const TradingCalendar& calendar)
{
    auto life = validate_instrument_life(instrument_start, instrument_end);
    if (!life) return life;
    if (!is_supported_date(observation_date))
        return std::unexpected(Error{ErrorCategory::invalid_date, "observation date is invalid"});
    if (observation_date < instrument_start || instrument_end < observation_date)
        return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                     "observation Date must be within the instrument life"});
    if (!calendar.is_trading_day(observation_date)) {
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "observation Date is not a trading day"});
    }
    return {};
}

/// Validates ordering, life bounds, and trading-day status for observation dates.
/// @return Success, or `invalid_date`, `invalid_time_range`, or `invalid_schedule`.
[[nodiscard]] inline Result<void> validate_observation_dates(
    std::span<const Date> observation_dates, Date instrument_start, Date instrument_end,
    const TradingCalendar& calendar)
{
    auto schedule = validate_date_schedule(observation_dates, instrument_start, instrument_end);
    if (!schedule) return schedule;
    for (const Date observation_date : observation_dates)
        if (!calendar.is_trading_day(observation_date))
            return std::unexpected(Error{ErrorCategory::invalid_date,
                                         "observation Date is not a trading day"});
    return {};
}

/// Immutable, strictly ordered collection of contract observation dates.
class ObservationSchedule {
public:
    /// Returns the underlying ordered dates.
    const std::vector<Date>& dates() const noexcept { return dates_; }
    /// Returns the number of dates.
    std::size_t size() const noexcept { return dates_.size(); }
    /// Returns whether the schedule has no dates.
    bool empty() const noexcept { return dates_.empty(); }
    /// Returns the date at `index` without bounds checking.
    const Date& operator[](std::size_t index) const noexcept { return dates_[index]; }
    /// Returns an iterator to the first date.
    auto begin() const noexcept { return dates_.begin(); }
    /// Returns the past-the-end iterator.
    auto end() const noexcept { return dates_.end(); }

    /// Compares the ordered date sequences.
    friend bool operator==(const ObservationSchedule&, const ObservationSchedule&) = default;

private:
    explicit ObservationSchedule(std::vector<Date> dates) : dates_(std::move(dates)) {}
    std::vector<Date> dates_;

    friend Result<ObservationSchedule> detail::make_date_schedule(std::vector<Date>, Date, Date);
    friend Result<ObservationSchedule> detail::make_observation_schedule(
        std::vector<Date>, Date, Date, const TradingCalendar&);
};

[[nodiscard]] inline Result<ObservationSchedule> detail::make_date_schedule(
    std::vector<Date> observation_dates, Date instrument_start, Date instrument_end)
{
    auto valid = validate_date_schedule(observation_dates, instrument_start, instrument_end);
    if (!valid) return std::unexpected(valid.error());
    return ObservationSchedule{std::move(observation_dates)};
}

[[nodiscard]] inline Result<ObservationSchedule> detail::make_observation_schedule(
    std::vector<Date> observation_dates, Date instrument_start, Date instrument_end,
    const TradingCalendar& calendar)
{
    auto valid = validate_observation_dates(observation_dates, instrument_start, instrument_end, calendar);
    if (!valid) return std::unexpected(valid.error());
    return ObservationSchedule{std::move(observation_dates)};
}

} // namespace kiyosi
