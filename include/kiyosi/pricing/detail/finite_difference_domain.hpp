#pragma once

#include <algorithm>
#include <cmath>
#include <concepts>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi::detail {

template <typename Option, typename Context>
double highest_finite_difference_level(const Option& option, const Context& context)
{
    double relevant = context.spot_price();
    if constexpr (requires { option.strike(); }) relevant = std::max(relevant, option.strike());
    if constexpr (requires { option.initial_spot(); option.upper_strike(); option.lower_strike(); })
        relevant = std::max({relevant, option.initial_spot(), option.upper_strike(), option.lower_strike()});
    if constexpr (requires { option.knock_out_level(); }) relevant = std::max(relevant, option.knock_out_level());
    if constexpr (requires { option.observations_view(); })
        for (const auto& observation : option.observations_view())
            relevant = std::max({relevant, observation.knock_out_level, observation.coupon_barrier_level});
    if constexpr (requires { option.knock_in_level(); }) relevant = std::max(relevant, option.knock_in_level());
    return relevant;
}

template <typename Option, typename Context>
double default_finite_difference_upper_boundary(const Option& option, const Context& context)
{
    const double relevant = highest_finite_difference_level(option, context);
    if constexpr (std::same_as<Option, AmericanOption>) {
        const auto& parameters = context.model_parameters();
        const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
        const double sigma = parameters.volatility();
        const double drift = parameters.risk_free_rate() - parameters.dividend_yield() - 0.5 * sigma * sigma;
        const double width = std::max(std::log(4.0), 4.0 * sigma * std::sqrt(time) + std::abs(drift) * time);
        return std::exp(std::log(relevant) + width);
    }
    if constexpr (requires { option.barrier_level(); })
        return std::max(4.0 * relevant, option.barrier_level() + relevant);
    return 4.0 * relevant;
}
} // namespace kiyosi::detail
