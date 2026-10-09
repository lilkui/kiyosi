#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Closed-form Black-Scholes-Merton engine for European vanilla options.
class KIYOSI_EXPORT AnalyticVanillaEngine {
public:
    /// Computes only price; returns intrinsic value at expiry_date.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(
        const EuropeanOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const EuropeanOption& option, const PricingContext& context,
        GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

private:
    [[nodiscard]] Result<PricingResult> price_native(
        const EuropeanOption&, const PricingContext&, GreeksRequest) const;
};

} // namespace kiyosi
