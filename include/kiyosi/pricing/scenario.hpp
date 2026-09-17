#pragma once

#include <span>
#include <utility>
#include <vector>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/revaluation.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Immutable column-oriented scenario results.
/// Column references remain valid until this value is moved from, assigned, or destroyed.
class ScenarioGridResult {
public:
    ScenarioGridResult(const ScenarioGridResult&) = default;
    ScenarioGridResult(ScenarioGridResult&&) noexcept = default;
    ~ScenarioGridResult() = default;

    ScenarioGridResult& operator=(ScenarioGridResult other) noexcept
    {
        spots_.swap(other.spots_);
        prices_.swap(other.prices_);
        deltas_.swap(other.deltas_);
        gammas_.swap(other.gammas_);
        return *this;
    }

    const std::vector<double>& spots() const noexcept { return spots_; }
    const std::vector<double>& prices() const noexcept { return prices_; }
    const std::vector<double>& deltas() const noexcept { return deltas_; }
    const std::vector<double>& gammas() const noexcept { return gammas_; }

private:
    template <typename Engine, typename Option>
    friend result<ScenarioGridResult> scenario_grid(
        const Engine&, const Option&, const PricingContext&, std::span<const double>,
        NumericalShiftSettings);

    ScenarioGridResult(std::vector<double> spots, std::vector<double> prices,
                       std::vector<double> deltas, std::vector<double> gammas)
        : spots_(std::move(spots)), prices_(std::move(prices)),
          deltas_(std::move(deltas)), gammas_(std::move(gammas))
    {
    }

    std::vector<double> spots_;
    std::vector<double> prices_;
    std::vector<double> deltas_;
    std::vector<double> gammas_;
};

/// Revalues price, delta, and gamma across a spot ladder, holding every other input fixed.
template <typename Engine, typename Option>
[[nodiscard]] result<ScenarioGridResult> scenario_grid(
    const Engine& engine, const Option& option, const PricingContext& context,
    std::span<const double> spots, NumericalShiftSettings settings = {})
{
    std::vector<double> scenario_spots{spots.begin(), spots.end()};
    std::vector<double> prices;
    std::vector<double> deltas;
    std::vector<double> gammas;
    prices.reserve(spots.size());
    deltas.reserve(spots.size());
    gammas.reserve(spots.size());
    for (double spot : spots) {
        auto shifted = detail::shifted_context(context, spot, context.parameters().volatility(),
                                               context.parameters().risk_free_rate(),
                                               context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        auto analytics = numerical_analytics(engine, option, *shifted, settings);
        if (!analytics) return std::unexpected(analytics.error());
        prices.push_back(*analytics->require(risk_measure::price));
        deltas.push_back(*analytics->require(risk_measure::delta));
        gammas.push_back(*analytics->require(risk_measure::gamma));
    }
    return ScenarioGridResult{std::move(scenario_spots), std::move(prices),
                              std::move(deltas), std::move(gammas)};
}

template <typename Engine, typename Option>
[[nodiscard]] result<ScenarioGridResult> scenario_grid(
    const Engine& engine, const Option& option, const PricingContext& context,
    const std::vector<double>& spots, NumericalShiftSettings settings = {})
{
    return scenario_grid(engine, option, context, std::span<const double>{spots}, settings);
}

} // namespace kiyosi
