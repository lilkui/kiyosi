#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/implied.hpp>

namespace kiyosi {

class KIYOSI_EXPORT AnalyticEuropeanEngine {
public:
    /// Returns intrinsic value with Greeks unavailable when valued at expiry.
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption& option, const PricingContext& context) const;

    /// Halley-with-bisection-fallback inversion that also enforces the arbitrage bounds.
    [[nodiscard]] result<double> implied_volatility(
        const EuropeanOption& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const;
};

} // namespace kiyosi
