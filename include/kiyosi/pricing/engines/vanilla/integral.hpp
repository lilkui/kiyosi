#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Simpson quadrature over the terminal lognormal density.
class KIYOSI_EXPORT QuadratureVanillaEngine {
public:
    /// Prices a European vanilla option by numerical quadrature.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(
        const EuropeanOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const EuropeanOption& option, const PricingContext& context,
        GreeksRequest greeks, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, greeks, settings,
                                         [&](const auto& engine) {
                                             return engine.price_native(option, context);
                                         });
    }

private:
    [[nodiscard]] Result<PricingResult> price_native(
        const EuropeanOption&, const PricingContext&) const;
};

} // namespace kiyosi
