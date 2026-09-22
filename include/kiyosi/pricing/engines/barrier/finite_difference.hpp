#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference engine for barrier options.
class KIYOSI_EXPORT FiniteDifferenceBarrierEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceBarrierEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceBarrierEngine(int asset_step_count, int time_step_count,
                                  FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices a barrier option by finite differences.
    /// @return Pricing measures, or a contract, context, or settings error.
    [[nodiscard]] Result<PricingResult> price(const BarrierOption& option, const PricingContext& context) const;
    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
