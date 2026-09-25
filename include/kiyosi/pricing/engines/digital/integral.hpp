#pragma once

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Simpson quadrature over the terminal lognormal density.
class KIYOSI_EXPORT QuadratureDigitalEngine {
public:
    /// Prices a cash-or-nothing option by numerical quadrature.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const CashOrNothingOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const CashOrNothingOption& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }

    /// Prices an asset-or-nothing option by numerical quadrature.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const AssetOrNothingOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AssetOrNothingOption& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }

private:
    [[nodiscard]] Result<PricingResult> price_native(const CashOrNothingOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<PricingResult> price_native(const AssetOrNothingOption& option, const PricingContext& context) const;
};

} // namespace kiyosi
