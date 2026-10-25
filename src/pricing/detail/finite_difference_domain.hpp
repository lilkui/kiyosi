#pragma once

#include <algorithm>
#include <cmath>
#include <concepts>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/pricing/detail/pricing.hpp>

namespace kiyosi::detail {

template <typename Option>
double highest_finite_difference_level(const Option& option)
{
    double relevant = 0.0;
    if constexpr (requires { option.strike(); }) relevant = std::max(relevant, option.strike());
    else if constexpr (requires { option.barrier_level(); }) relevant = std::max(relevant, option.barrier_level());
    if constexpr (requires { option.initial_spot(); option.upper_strike(); option.lower_strike(); })
        relevant = std::max({relevant, option.initial_spot(), option.upper_strike(), option.lower_strike()});
    if constexpr (requires { option.knock_out_level(); }) relevant = std::max(relevant, option.knock_out_level());
    if constexpr (requires { option.observations_view(); })
        for (const auto& observation : option.observations_view())
            relevant = std::max({relevant, observation.knock_out_level, observation.coupon_barrier_level});
    if constexpr (requires { option.knock_in_level(); }) relevant = std::max(relevant, option.knock_in_level());
    if constexpr (requires { option.upper_barrier(); }) relevant = std::max(relevant, option.upper_barrier());
    return relevant;
}

template <typename Option, typename Context>
double highest_finite_difference_level(const Option& option, const Context& context)
{
    return std::max(context.spot_price(), highest_finite_difference_level(option));
}

template <typename Context>
double volatility_adjusted_upper_boundary(double relevant, Date expiry, const Context& context)
{
    const auto& parameters = context.model_parameters();
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), expiry);
    const double sigma = parameters.volatility();
    if (parameters.has_flat_rates()) {
        const double drift = *parameters.risk_free_curve().flat_rate() - *parameters.dividend_curve().flat_rate() - 0.5 * sigma * sigma;
        const double width = 4.0 * sigma * std::sqrt(time) + std::abs(drift) * time;
        return width <= std::log(4.0) ? 4.0 * relevant : std::exp(std::log(relevant) + width);
    }
    double maximum_drift = 0.0;
    auto times = curve_times(context, start_of_day(expiry));
    times.push_back(time);
    for (const double end : times) {
        const double drift = -curve_log_factor(parameters.risk_free_curve(), context.valuation_time(), 0.0, end) +
                             curve_log_factor(parameters.dividend_curve(), context.valuation_time(), 0.0, end) - 0.5 * sigma * sigma * end;
        maximum_drift = std::max(maximum_drift, std::abs(drift));
    }
    const double width = 4.0 * sigma * std::sqrt(time) + maximum_drift;
    return width <= std::log(4.0) ? 4.0 * relevant : std::exp(std::log(relevant) + width);
}

template <typename Option, typename Context>
double default_finite_difference_upper_boundary(const Option& option, const Context& context)
{
    const double relevant = highest_finite_difference_level(option, context);
    if constexpr (std::same_as<Option, AmericanOption> || requires { option.observations_view(); }) {
        return volatility_adjusted_upper_boundary(relevant, option.expiry_date(), context);
    }
    if constexpr (requires { option.barrier_level(); })
        return std::max(4.0 * relevant, option.barrier_level() + relevant);
    return 4.0 * relevant;
}
} // namespace kiyosi::detail
