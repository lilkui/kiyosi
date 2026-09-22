#pragma once

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Marches the accrual value back as an affine function of the accumulated quantity, so a single
/// sweep prices every opening position.
class KIYOSI_EXPORT FiniteDifferenceAccumulatorEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceAccumulatorEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceAccumulatorEngine(int asset_step_count, int time_step_count,
                                      FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices an accumulator by finite differences.
    /// @return Pricing measures, or a contract, context, or settings error.
    [[nodiscard]] Result<PricingResult> price(const Accumulator&, const PricingContext&) const;
    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
