#pragma once

#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

enum class BarrierType {
    up_and_in,
    up_and_out,
    down_and_in,
    down_and_out,
};

enum class ObservationMode { continuous,
                              scheduled };

enum class RebateTiming { at_hit,
                           at_expiry };

enum class SettlementTiming { at_hit,
                               at_expiry };

[[nodiscard]] constexpr bool is_up_barrier(BarrierType kind) noexcept
{
    return kind == BarrierType::up_and_in || kind == BarrierType::up_and_out;
}

[[nodiscard]] constexpr bool is_knock_in_barrier(BarrierType kind) noexcept
{
    return kind == BarrierType::up_and_in || kind == BarrierType::down_and_in;
}

class BarrierTerms;

namespace detail {
[[nodiscard]] Result<BarrierTerms> make_barrier_terms(
    double, BarrierType, ObservationMode, std::vector<Date>, Date, Date);
}

/// Trigger level, knock direction, and monitoring schedule shared by every barrier contract.
class BarrierTerms {
public:
    double barrier_level() const noexcept { return barrier_level_; }
    BarrierType barrier_type() const noexcept { return barrier_type_; }
    kiyosi::ObservationMode observation_mode() const noexcept { return observation_mode_; }
    const ObservationSchedule& observation_schedule() const noexcept { return observation_dates_; }
    const std::vector<Date>& observation_dates() const noexcept { return observation_dates_.dates(); }
    Date effective_date() const noexcept { return effective_date_; }
    Date expiry_date() const noexcept { return expiry_date_; }

    bool is_up() const noexcept { return is_up_barrier(barrier_type_); }
    bool is_knock_in() const noexcept { return is_knock_in_barrier(barrier_type_); }
    bool is_continuous() const noexcept { return observation_mode_ == ObservationMode::continuous; }

    /// Mean spacing between monitoring dates, used for the BGK discrete-barrier shift.
    double mean_observation_year_fraction() const noexcept
    {
        return is_continuous() || observation_dates_.empty()
                   ? 0.0
                   : *year_fraction(effective_date_, observation_dates_.dates().back()) /
                         static_cast<double>(observation_dates_.size());
    }

    /// True when the barrier is being monitored on `date`.
    bool is_monitored_on(Date date) const noexcept
    {
        if (is_continuous()) return true;
        for (const Date event : observation_dates_.dates())
            if (event == date) return true;
        return false;
    }

    /// True when `spot` sits on the knocked side of the barrier.
    bool is_breached_by(double spot) const noexcept
    {
        return is_up() ? spot >= barrier_level_ : spot <= barrier_level_;
    }

    friend bool operator==(const BarrierTerms&, const BarrierTerms&) = default;

private:
    BarrierTerms(double barrier_level, BarrierType barrier_type, kiyosi::ObservationMode observation_mode,
                 ObservationSchedule observation_dates, Date effective_date, Date expiry_date)
        : barrier_level_(barrier_level), barrier_type_(barrier_type), observation_mode_(observation_mode),
          observation_dates_(std::move(observation_dates)), effective_date_(effective_date), expiry_date_(expiry_date) {}

    double barrier_level_;
    BarrierType barrier_type_;
    kiyosi::ObservationMode observation_mode_;
    ObservationSchedule observation_dates_;
    Date effective_date_;
    Date expiry_date_;

    friend Result<BarrierTerms> detail::make_barrier_terms(
        double, BarrierType, kiyosi::ObservationMode, std::vector<Date>, Date, Date);
};

[[nodiscard]] inline Result<BarrierTerms> detail::make_barrier_terms(
    double barrier_level, BarrierType barrier_type, kiyosi::ObservationMode observation_mode,
    std::vector<Date> observation_dates,
    Date effective_date, Date expiry_date)
{
    if (!std::isfinite(barrier_level) || barrier_level <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "barrier terms must be finite and non-negative"});
    if (!is_supported_date(effective_date) || !is_supported_date(expiry_date) || effective_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "barrier life dates are invalid"});
    if (barrier_type != BarrierType::up_and_in && barrier_type != BarrierType::up_and_out &&
        barrier_type != BarrierType::down_and_in && barrier_type != BarrierType::down_and_out)
        return std::unexpected(Error{ErrorCategory::invalid_option, "invalid barrier type"});
    if (observation_mode != kiyosi::ObservationMode::continuous &&
        observation_mode != kiyosi::ObservationMode::scheduled)
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "invalid observation mode"});
    if (observation_mode == kiyosi::ObservationMode::continuous && !observation_dates.empty())
        return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                     "continuous barriers cannot have observation dates"});
    if (observation_mode == kiyosi::ObservationMode::scheduled && observation_dates.empty())
        return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                     "scheduled barriers require observation dates"});
    auto schedule = detail::make_date_schedule(std::move(observation_dates), effective_date, expiry_date);
    if (!schedule)
        return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                     "observation dates must be ordered and not exceed expiry_date"});
    return BarrierTerms{barrier_level, barrier_type, observation_mode, std::move(*schedule), effective_date, expiry_date};
}

} // namespace kiyosi
