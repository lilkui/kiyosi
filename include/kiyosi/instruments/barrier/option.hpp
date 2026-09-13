#pragma once

#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/option_terms.hpp>

namespace kiyosi {

class BarrierOption;

[[nodiscard]] result<BarrierOption> make_barrier_option(
    option_type, double, date, date, double, barrier_type, double, rebate_timing, observation_mode,
    std::vector<date>);

/// Knock-in or knock-out vanilla payoff with an optional rebate.
class BarrierOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    double rebate() const noexcept { return rebate_; }
    kiyosi::rebate_timing rebate_payment() const noexcept { return timing_; }

    const BarrierTerms& barrier_terms() const noexcept { return barrier_; }
    double barrier() const noexcept { return barrier_.barrier(); }
    barrier_type barrier_kind() const noexcept { return barrier_.kind(); }
    observation_mode observation() const noexcept { return barrier_.observation(); }
    const ObservationSchedule& schedule() const noexcept { return barrier_.schedule(); }
    const std::vector<date>& observation_dates() const noexcept { return barrier_.observation_dates(); }
    double observation_interval() const noexcept { return barrier_.observation_interval(); }
    date effective() const noexcept { return barrier_.effective(); }
    date expiry() const noexcept { return barrier_.expiry(); }

    friend bool operator==(const BarrierOption&, const BarrierOption&) = default;

private:
    BarrierOption(option_type type, double strike, double rebate, kiyosi::rebate_timing timing,
                  BarrierTerms barrier)
        : type_(type), strike_(strike), rebate_(rebate), timing_(timing), barrier_(std::move(barrier)) {}

    option_type type_;
    double strike_;
    double rebate_;
    kiyosi::rebate_timing timing_;
    BarrierTerms barrier_;

    friend result<BarrierOption> make_barrier_option(
        option_type, double, date, date, double, barrier_type, double, rebate_timing,
        observation_mode, std::vector<date>);
};

[[nodiscard]] inline result<BarrierOption> make_barrier_option(
    option_type type, double strike, date effective, date expiry, double barrier, barrier_type kind,
    double rebate = 0.0, rebate_timing timing = rebate_timing::at_expiry,
    observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    auto terms = detail::make_barrier_terms(barrier, kind, observation, std::move(observations),
                                            effective, expiry);
    if (!terms) return std::unexpected(terms.error());
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(rebate) || rebate < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "barrier terms must be finite and non-negative"});
    if (timing != rebate_timing::at_hit && timing != rebate_timing::at_expiry)
        return std::unexpected(Error{error_category::invalid_option, "invalid rebate timing"});
    if (terms->is_knock_in() && timing == rebate_timing::at_hit)
        return std::unexpected(Error{error_category::invalid_option,
                                     "at-hit rebates are invalid for knock-in barriers"});
    return BarrierOption{type, strike, rebate, timing, std::move(*terms)};
}

[[nodiscard]] inline result<BarrierOption> make_barrier_option(
    option_type type, double strike, date effective, date expiry, double barrier, barrier_type kind,
    double rebate, rebate_timing timing, observation_mode observation, ObservationSchedule schedule)
{
    return make_barrier_option(type, strike, effective, expiry, barrier, kind, rebate, timing,
                               observation, schedule.dates());
}

} // namespace kiyosi
