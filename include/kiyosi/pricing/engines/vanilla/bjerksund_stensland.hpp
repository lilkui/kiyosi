#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Bjerksund-Stensland (2002) two-step American approximation.
/// Before expiry, calls require a non-negative risk-free rate and puts require a
/// non-negative dividend yield (the interest rate under put-call symmetry).
class KIYOSI_EXPORT BjerksundStenslandVanillaEngine {
public:
    /// Prices an American vanilla option with the two-step approximation.
    /// @return Price, a contract or context error, or `unsupported_operation` for a negative transformed rate.
    [[nodiscard]] Result<double> price(
        const AmericanOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const AmericanOption& option, const PricingContext& context,
        GreeksRequest greeks, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, greeks, settings,
                                         [&](const auto& engine) {
                                             return engine.price_native(option, context);
                                         });
    }

private:
    [[nodiscard]] Result<PricingResult> price_native(
        const AmericanOption&, const PricingContext&) const;
};

} // namespace kiyosi
