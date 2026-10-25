#pragma once

#include <algorithm>
#include <cmath>
#include <ranges>
#include <vector>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi::detail {

[[nodiscard]] inline Result<void> require_flat_rates(const PricingContext& context)
{
    if (!context.model_parameters().has_flat_rates())
        return std::unexpected(Error{ErrorCategory::unsupported_operation, "this engine requires scalar or explicitly flat rate curves"});
    return {};
}

[[nodiscard]] inline Result<void> validate_curve_horizon(const PricingContext& context, Timestamp expiry)
{
    // Pricing validates life; a flat curve covering valuation also covers every later timestamp.
    for (const auto* curve : {&context.model_parameters().risk_free_curve(), &context.model_parameters().dividend_curve()})
        if (!curve->is_flat())
            if (auto valid = RateTermStructureAccess::validate_horizon(*curve, context.valuation_time(), expiry); !valid)
                return std::unexpected(valid.error());
    return {};
}

inline double curve_log_factor(const RateTermStructure& curve, Timestamp origin, double start, double end) noexcept
{
    return RateTermStructureAccess::log_factor(curve, origin, start, end);
}

inline double curve_forward_rate(const RateTermStructure& curve, Timestamp origin, double time) noexcept
{
    return RateTermStructureAccess::forward_rate(curve, origin, time);
}

/// Strictly interior knots in years from valuation, for prevalidated pricing horizons.
inline std::vector<double> curve_times(const PricingContext& context, Timestamp expiry)
{
    std::vector<double> times;
    for (const auto* curve : {&context.model_parameters().risk_free_curve(), &context.model_parameters().dividend_curve()})
        for (const Timestamp time : curve->nodes())
            if (time > context.valuation_time() && time < expiry)
                times.push_back(actual_365_fixed_year_fraction(context.valuation_time(), time));
    std::ranges::sort(times);
    times.erase(std::ranges::unique(times).begin(), times.end());
    return times;
}

/// Scalar engines use absolute rate bumps; curve-aware completion uses zero as its shift origin.
inline double rate_shift_origin(const BlackScholesMertonParameters& parameters) noexcept
{
    return parameters.risk_free_curve().flat_rate().value_or(0.0);
}

template <typename Engine, typename Option>
[[nodiscard]] Result<double> numerical_value(
    const Engine& engine, const Option& option, const PricingContext& context)
{
    auto priced = engine.price(option, context);
    if (!priced) return std::unexpected(priced.error());
    return checked_price(*priced);
}

[[nodiscard]] inline Result<PricingContext> shifted_context(
    const PricingContext& context, double spot, double volatility, double rate, Timestamp valuation)
{
    const auto shifted_rate = context.model_parameters().risk_free_curve().shifted(rate - rate_shift_origin(context.model_parameters()));
    if (!shifted_rate) return std::unexpected(shifted_rate.error());
    auto parameters = make_bsm_parameters(*shifted_rate, context.model_parameters().dividend_curve(), volatility);
    if (!parameters) return std::unexpected(parameters.error());
    return make_pricing_context(*parameters, spot, valuation, context.calendar());
}

} // namespace kiyosi::detail
