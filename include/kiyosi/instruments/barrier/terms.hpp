#pragma once

#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

enum class barrier_type {
    up_and_in,
    up_and_out,
    down_and_in,
    down_and_out,
};

enum class observation_mode { continuous,
                              scheduled };

enum class rebate_timing { at_hit,
                           at_expiry };

enum class settlement_timing { at_hit,
                               at_expiry };

[[nodiscard]] constexpr bool is_up_barrier(barrier_type kind) noexcept
{
    return kind == barrier_type::up_and_in || kind == barrier_type::up_and_out;
}

[[nodiscard]] constexpr bool is_knock_in_barrier(barrier_type kind) noexcept
{
    return kind == barrier_type::up_and_in || kind == barrier_type::down_and_in;
}

class BarrierTerms;

namespace detail {
[[nodiscard]] result<BarrierTerms> make_barrier_terms(
    double, barrier_type, observation_mode, std::vector<date>, date, date);
}

/// Trigger level, knock direction, and monitoring schedule shared by every barrier contract.
class BarrierTerms {
public:
    double barrier() const noexcept { return barrier_; }
    barrier_type kind() const noexcept { return kind_; }
    observation_mode observation() const noexcept { return observation_; }
    const ObservationSchedule& schedule() const noexcept { return observations_; }
    const std::vector<date>& observation_dates() const noexcept { return observations_.dates(); }
    date effective() const noexcept { return effective_; }
    date expiry() const noexcept { return expiry_; }

    bool is_up() const noexcept { return is_up_barrier(kind_); }
    bool is_knock_in() const noexcept { return is_knock_in_barrier(kind_); }
    bool is_continuous() const noexcept { return observation_ == observation_mode::continuous; }

    /// Mean spacing between monitoring dates, used for the BGK discrete-barrier shift.
    double observation_interval() const noexcept
    {
        return is_continuous() || observations_.empty()
                   ? 0.0
                   : *year_fraction(effective_, observations_.dates().back()) /
                         static_cast<double>(observations_.size());
    }

    /// True when the barrier is being monitored at `moment`.
    bool monitors(timestamp moment) const noexcept
    {
        if (is_continuous()) return true;
        for (const date event : observations_.dates())
            if (event == moment) return true;
        return false;
    }

    /// True when `spot` sits on the knocked side of the barrier.
    bool breaches(double spot) const noexcept
    {
        return is_up() ? spot >= barrier_ : spot <= barrier_;
    }

    friend bool operator==(const BarrierTerms&, const BarrierTerms&) = default;

private:
    BarrierTerms(double barrier, barrier_type kind, observation_mode observation,
                 ObservationSchedule observations, date effective, date expiry)
        : barrier_(barrier), kind_(kind), observation_(observation),
          observations_(std::move(observations)), effective_(effective), expiry_(expiry) {}

    double barrier_;
    barrier_type kind_;
    observation_mode observation_;
    ObservationSchedule observations_;
    date effective_;
    date expiry_;

    friend result<BarrierTerms> detail::make_barrier_terms(
        double, barrier_type, observation_mode, std::vector<date>, date, date);
};

[[nodiscard]] inline result<BarrierTerms> detail::make_barrier_terms(
    double barrier, barrier_type kind, observation_mode observation, std::vector<date> observations,
    date effective, date expiry)
{
    if (!std::isfinite(barrier) || barrier <= 0.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "barrier terms must be finite and non-negative"});
    if (!is_valid_date(effective) || !is_valid_date(expiry) || effective > expiry)
        return std::unexpected(Error{error_category::invalid_schedule, "barrier life dates are invalid"});
    if (kind != barrier_type::up_and_in && kind != barrier_type::up_and_out &&
        kind != barrier_type::down_and_in && kind != barrier_type::down_and_out)
        return std::unexpected(Error{error_category::invalid_option, "invalid barrier type"});
    if (observation != observation_mode::continuous && observation != observation_mode::scheduled)
        return std::unexpected(Error{error_category::invalid_schedule, "invalid observation mode"});
    if (observation == observation_mode::continuous && !observations.empty())
        return std::unexpected(Error{error_category::invalid_schedule,
                                     "continuous barriers cannot have observations"});
    if (observation == observation_mode::scheduled && observations.empty())
        return std::unexpected(Error{error_category::invalid_schedule,
                                     "scheduled barriers require observations"});
    auto schedule = detail::make_date_schedule(std::move(observations), effective, expiry);
    if (!schedule)
        return std::unexpected(Error{error_category::invalid_schedule,
                                     "observation dates must be ordered and precede expiry"});
    return BarrierTerms{barrier, kind, observation, std::move(*schedule), effective, expiry};
}

} // namespace kiyosi
