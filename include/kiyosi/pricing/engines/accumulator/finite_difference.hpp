#pragma once

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Marches the accrual value back as an affine function of the accumulated quantity, so a single
/// sweep prices every opening position.
/// Accepts 3..2,000 asset steps and 1..2,000 time steps.
class KIYOSI_EXPORT FiniteDifferenceAccumulatorEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceAccumulatorEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceAccumulatorEngine(int asset_step_count, int time_step_count,
                                      FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices an accumulator by finite differences.
    /// @return Price, or a contract, context, or settings error.
    [[nodiscard]] Result<double> price(const Accumulator& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// Delta and gamma reuse the solved layers; vanna and zomma difference shifted native delta and gamma.
    /// Other measures use numerical price differences.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const Accumulator& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_native(
        const Accumulator& option, const PricingContext& context, GreeksRequest output) const;

    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
