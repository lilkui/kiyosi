#pragma once

#include <cmath>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi::detail {

/// Prices `option` and extracts a finite price, or reports why no price is available.
template <typename Engine, typename Option>
[[nodiscard]] result<double> numerical_value(
    const Engine& engine, const Option& option, const PricingContext& context)
{
    auto priced = engine.price(option, context);
    if (!priced) return std::unexpected(priced.error());
    const auto value = priced->get(risk_measure::price);
    if (!value) return std::unexpected(value.error());
    if (!*value || !std::isfinite(**value))
        return std::unexpected(Error{error_category::invalid_result, "pricing produced no finite price"});
    return **value;
}

/// Rebuilds `context` with bumped market state, preserving the dividend yield and calendar.
[[nodiscard]] inline result<PricingContext> shifted_context(
    const PricingContext& context, double spot, double volatility, double rate, timestamp valuation)
{
    auto parameters = make_bsm_parameters(rate, context.parameters().dividend_yield(), volatility);
    if (!parameters) return std::unexpected(parameters.error());
    return make_pricing_context(*parameters, spot, valuation, context.calendar());
}

template <typename Engine, typename Option>
[[nodiscard]] result<double> shifted_value(
    const Engine& engine, const Option& option, const PricingContext& context, double spot,
    double volatility, double rate, timestamp valuation)
{
    auto shifted = shifted_context(context, spot, volatility, rate, valuation);
    if (!shifted) return std::unexpected(shifted.error());
    return numerical_value(engine, option, *shifted);
}

} // namespace kiyosi::detail
