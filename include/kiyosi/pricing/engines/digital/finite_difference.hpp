#pragma once

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference engine for European digital options.
/// Upper-edge values use BSM exercise probabilities at each time step.
/// Accepts 3..10,000 asset steps and 1..100,000 time steps.
/// Prices materially outside the non-negative payoff bounds return `invalid_result`.
class KIYOSI_EXPORT FiniteDifferenceDigitalEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceDigitalEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceDigitalEngine(int asset_step_count, int time_step_count,
                                  FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices a European cash-or-nothing or asset-or-nothing option.
    /// @return Price, or a contract, context, or settings error.
    [[nodiscard]] Result<double> price(const CashOrNothingOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AssetOrNothingOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const CashOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AssetOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_native(const CashOrNothingOption&, const PricingContext&, GreeksRequest) const;
    [[nodiscard]] Result<PricingResult> price_native(const AssetOrNothingOption&, const PricingContext&, GreeksRequest) const;
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
