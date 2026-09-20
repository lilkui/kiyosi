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

[[nodiscard]] inline Result<void> validate_date_schedule(
    std::span<const Date> observation_dates, Date instrument_start, Date instrument_end)
{
    if (!is_supported_date(instrument_start) || !is_supported_date(instrument_end) || instrument_end < instrument_start)
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "instrument life must be a valid ordered Date range"});
    for (std::size_t index = 0; index < observation_dates.size(); ++index) {
        if (index > 0 && observation_dates[index] <= observation_dates[index - 1])
            return std::unexpected(Error{ErrorCategory::invalid_date,
                                         "observation dates must be strictly ordered"});
        if (!is_supported_date(observation_dates[index]) || observation_dates[index] < instrument_start ||
            observation_dates[index] > instrument_end)
            return std::unexpected(Error{ErrorCategory::invalid_date,
                                         "observation Date must be within the instrument life"});
    }
    return {};
}

[[nodiscard]] inline Result<void> validate_observation_date(
    Date observation_date, Date instrument_start, Date instrument_end, const TradingCalendar& calendar)
{
    if (!is_supported_date(observation_date) || !is_supported_date(instrument_start) ||
        !is_supported_date(instrument_end) || instrument_end < instrument_start ||
        observation_date < instrument_start || instrument_end < observation_date) {
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "observation Date must be within the instrument life"});
    }
    if (!calendar.is_trading_day(observation_date)) {
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "observation Date is not a trading day"});
    }
    return {};
}

[[nodiscard]] inline Result<void> validate_observation_dates(
    std::span<const Date> observation_dates, Date instrument_start, Date instrument_end,
    const TradingCalendar& calendar)
{
    if (!is_supported_date(instrument_start) || !is_supported_date(instrument_end) || instrument_end < instrument_start) {
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "instrument life must be a valid ordered Date range"});
    }
    for (std::size_t index = 0; index < observation_dates.size(); ++index) {
        if (index > 0 && observation_dates[index] <= observation_dates[index - 1]) {
            return std::unexpected(Error{ErrorCategory::invalid_date,
                                         "observation dates must be strictly ordered"});
        }
        auto valid = validate_observation_date(observation_dates[index], instrument_start, instrument_end, calendar);
        if (!valid) return std::unexpected(valid.error());
    }
    return {};
}

class ObservationSchedule {
public:
    const std::vector<Date>& dates() const noexcept { return dates_; }
    std::size_t size() const noexcept { return dates_.size(); }
    bool empty() const noexcept { return dates_.empty(); }
    const Date& operator[](std::size_t index) const noexcept { return dates_[index]; }
    auto begin() const noexcept { return dates_.begin(); }
    auto end() const noexcept { return dates_.end(); }

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
