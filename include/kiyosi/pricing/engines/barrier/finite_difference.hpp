#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference engine for barrier options.
/// Accepts 3..10,000 asset steps and 1..100,000 time steps.
class KIYOSI_EXPORT FiniteDifferenceBarrierEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceBarrierEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceBarrierEngine(int asset_step_count, int time_step_count,
                                  FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices a barrier option by finite differences.
    /// @return Price, or a contract, context, or settings error.
    [[nodiscard]] Result<double> price(const BarrierOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_native(const BarrierOption& option, const PricingContext& context) const;
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
