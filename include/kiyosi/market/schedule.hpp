#pragma once

#include <span>
#include <utility>
#include <vector>

#include <kiyosi/core/time.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi {

class ObservationSchedule;

namespace detail {
[[nodiscard]] result<ObservationSchedule> make_date_schedule(std::vector<date>, date, date);
[[nodiscard]] result<ObservationSchedule> make_observation_schedule(
    std::vector<date>, date, date, const TradingCalendar&);
} // namespace detail

[[nodiscard]] inline result<void> validate_date_schedule(
    std::span<const date> observations, date instrument_start, date instrument_end)
{
    if (!is_valid_date(instrument_start) || !is_valid_date(instrument_end) || instrument_end < instrument_start)
        return std::unexpected(Error{error_category::invalid_date,
                                     "instrument life must be a valid ordered date range"});
    for (std::size_t index = 0; index < observations.size(); ++index) {
        if (index > 0 && observations[index] <= observations[index - 1])
            return std::unexpected(Error{error_category::invalid_date,
                                         "observation dates must be strictly ordered"});
        if (!is_valid_date(observations[index]) || observations[index] < instrument_start ||
            observations[index] > instrument_end)
            return std::unexpected(Error{error_category::invalid_date,
                                         "observation date must be within the instrument life"});
    }
    return {};
}

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

    friend result<ObservationSchedule> detail::make_date_schedule(std::vector<date>, date, date);
    friend result<ObservationSchedule> detail::make_observation_schedule(
        std::vector<date>, date, date, const TradingCalendar&);
};

[[nodiscard]] inline result<ObservationSchedule> detail::make_date_schedule(
    std::vector<date> observations, date instrument_start, date instrument_end)
{
    auto valid = validate_date_schedule(observations, instrument_start, instrument_end);
    if (!valid) return std::unexpected(valid.error());
    return ObservationSchedule{std::move(observations)};
}

[[nodiscard]] inline result<ObservationSchedule> detail::make_observation_schedule(
    std::vector<date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    auto valid = validate_observation_dates(observations, instrument_start, instrument_end, calendar);
    if (!valid) return std::unexpected(valid.error());
    return ObservationSchedule{std::move(observations)};
}

} // namespace kiyosi
