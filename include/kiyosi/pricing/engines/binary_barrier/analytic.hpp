#pragma once

#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Closed-form Rubinstein-Reiner binary barrier and touch valuation.
class KIYOSI_EXPORT AnalyticBinaryBarrierEngine {
public:
    /// Prices a strike-based binary barrier option.
    /// @return Price, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<double> price(const BinaryBarrierOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BinaryBarrierOption& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }

    /// Prices a one-touch or no-touch option.
    /// @return Price, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<double> price(const TouchOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const TouchOption& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }

private:
    [[nodiscard]] Result<PricingResult> price_native(const BinaryBarrierOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<PricingResult> price_native(const TouchOption& option, const PricingContext& context) const;
};

} // namespace kiyosi
