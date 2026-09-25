#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Closed-form Reiner-Rubinstein barrier valuation with a BGK shift for scheduled monitoring.
class KIYOSI_EXPORT AnalyticBarrierEngine {
public:
    /// Prices a barrier option analytically.
    /// @return Price, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<double> price(const BarrierOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BarrierOption& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }

private:
    [[nodiscard]] Result<PricingResult> price_native(const BarrierOption& option, const PricingContext& context) const;
};

} // namespace kiyosi
