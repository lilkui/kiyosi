#pragma once

#include <span>
#include <vector>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/revaluation.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

struct ScenarioGridResult {
    std::vector<double> spots;
    std::vector<double> prices;
    std::vector<double> deltas;
    std::vector<double> gammas;
};

/// Revalues price, delta, and gamma across a spot ladder, holding every other input fixed.
template <typename Engine, typename Option>
[[nodiscard]] result<ScenarioGridResult> scenario_grid(
    const Engine& engine, const Option& option, const PricingContext& context,
    std::span<const double> spots, NumericalShiftSettings settings = {})
{
    ScenarioGridResult result{.spots = std::vector<double>{spots.begin(), spots.end()}};
    result.prices.reserve(spots.size());
    result.deltas.reserve(spots.size());
    result.gammas.reserve(spots.size());
    for (double spot : spots) {
        auto shifted = detail::shifted_context(context, spot, context.parameters().volatility(),
                                               context.parameters().risk_free_rate(),
                                               context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        auto analytics = numerical_analytics(engine, option, *shifted, settings);
        if (!analytics) return std::unexpected(analytics.error());
        result.prices.push_back(*analytics->require(risk_measure::price));
        result.deltas.push_back(*analytics->require(risk_measure::delta));
        result.gammas.push_back(*analytics->require(risk_measure::gamma));
    }
    return result;
}

template <typename Engine, typename Option>
[[nodiscard]] result<ScenarioGridResult> scenario_grid(
    const Engine& engine, const Option& option, const PricingContext& context,
    const std::vector<double>& spots, NumericalShiftSettings settings = {})
{
    return scenario_grid(engine, option, context, std::span<const double>{spots}, settings);
}

} // namespace kiyosi
