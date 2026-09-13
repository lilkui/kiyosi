#pragma once

#include <vector>
#include <kiyosi/core/types.hpp>
#include <kiyosi/core/schedule.hpp>
#include <kiyosi/instruments/option_terms.hpp>

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

class BarrierOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    double barrier() const noexcept { return barrier_; }
    barrier_type barrier_kind() const noexcept { return kind_; }
    double rebate() const noexcept { return rebate_; }
    kiyosi::rebate_timing rebate_payment() const noexcept { return timing_; }
    observation_mode observation() const noexcept { return observation_; }
    const std::vector<date>& observation_dates() const noexcept { return observations_.dates(); }
    const ObservationSchedule& schedule() const noexcept { return observations_; }
    date expiry() const noexcept { return expiry_; }
    date effective() const noexcept { return effective_; }
    double observation_interval() const noexcept
    {
        return observation_ == observation_mode::continuous || observations_.empty()
                   ? 0.0
                   : static_cast<double>(*year_fraction(effective_, observations_.dates().back())) /
                         static_cast<double>(observations_.size());
    }
    friend bool operator==(const BarrierOption&, const BarrierOption&) = default;

private:
    BarrierOption(option_type type, double strike, date effective, date expiry, double barrier, barrier_type kind,
                  double rebate, kiyosi::rebate_timing timing, observation_mode observation,
                  ObservationSchedule observations)
        : type_(type), strike_(strike), barrier_(barrier), kind_(kind), rebate_(rebate), timing_(timing),
          observation_(observation), observations_(std::move(observations)), effective_(effective), expiry_(expiry) {}
    option_type type_;
    double strike_;
    double barrier_;
    barrier_type kind_;
    double rebate_;
    kiyosi::rebate_timing timing_;
    observation_mode observation_;
    ObservationSchedule observations_;
    date effective_;
    date expiry_;
    friend result<BarrierOption> make_barrier_option(option_type, double, date, date, double, barrier_type,
                                                     double, kiyosi::rebate_timing, observation_mode, std::vector<date>);
    friend result<BarrierOption> make_barrier_option(
        option_type, double, date, date, double, barrier_type, double, rebate_timing,
        observation_mode, ObservationSchedule);
};
[[nodiscard]] inline result<BarrierOption> make_barrier_option(
    option_type type, double strike, date effective, date expiry, double barrier, barrier_type kind,
    double rebate = 0.0, rebate_timing timing = rebate_timing::at_expiry,
    observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(barrier) || barrier <= 0.0 || !std::isfinite(rebate) || rebate < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "barrier terms must be finite and non-negative"});
    if (!is_valid_date(effective) || !is_valid_date(expiry) || effective > expiry)
        return std::unexpected(Error{error_category::invalid_schedule, "barrier life dates are invalid"});
    if (kind != barrier_type::up_and_in && kind != barrier_type::up_and_out &&
        kind != barrier_type::down_and_in && kind != barrier_type::down_and_out)
        return std::unexpected(Error{error_category::invalid_option, "invalid barrier type"});
    if (timing != rebate_timing::at_hit && timing != rebate_timing::at_expiry)
        return std::unexpected(Error{error_category::invalid_option, "invalid rebate timing"});
    if (observation != observation_mode::continuous && observation != observation_mode::scheduled)
        return std::unexpected(Error{error_category::invalid_schedule, "invalid observation mode"});
    const bool knock_in = kind == barrier_type::up_and_in || kind == barrier_type::down_and_in;
    if (knock_in && timing == rebate_timing::at_hit)
        return std::unexpected(Error{error_category::invalid_option, "at-hit rebates are invalid for knock-in barriers"});
    if (observation == observation_mode::continuous && !observations.empty())
        return std::unexpected(Error{error_category::invalid_schedule, "continuous barriers cannot have observations"});
    if (observation == observation_mode::scheduled) {
        if (observations.empty())
            return std::unexpected(Error{error_category::invalid_schedule, "scheduled barriers require observations"});
    }
    if (observation == observation_mode::continuous)
        return BarrierOption{type, strike, effective, expiry, barrier, kind, rebate, timing, observation,
                             *detail::make_date_schedule({}, effective, expiry)};
    auto schedule = detail::make_date_schedule(std::move(observations), effective, expiry);
    if (!schedule)
        return std::unexpected(Error{error_category::invalid_schedule,
                                     "observation dates must be ordered and precede expiry"});
    return BarrierOption{type, strike, effective, expiry, barrier, kind, rebate, timing, observation, std::move(*schedule)};
}

[[nodiscard]] inline result<BarrierOption> make_barrier_option(
    option_type type, double strike, date effective, date expiry, double barrier, barrier_type kind,
    double rebate, rebate_timing timing, observation_mode observation, ObservationSchedule schedule)
{
    if (observation == observation_mode::continuous && !schedule.empty())
        return std::unexpected(Error{error_category::invalid_schedule, "continuous barriers cannot have observations"});
    if (observation == observation_mode::scheduled && schedule.empty())
        return std::unexpected(Error{error_category::invalid_schedule, "scheduled barriers require observations"});
    auto base = make_barrier_option(type, strike, effective, expiry, barrier, kind, rebate, timing,
                                    observation, schedule.dates());
    if (!base) return std::unexpected(base.error());
    base->observations_ = std::move(schedule);
    return base;
}

} // namespace kiyosi
