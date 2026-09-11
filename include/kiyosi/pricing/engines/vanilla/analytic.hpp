#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/engines/settings/implied_volatility.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

class KIYOSI_EXPORT AnalyticEuropeanEngine {
public:
    /// Returns intrinsic value with Greeks unavailable when valued at expiry.
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption& option, const PricingContext& context) const;

    [[nodiscard]] result<double> implied_volatility(
        const EuropeanOption& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const;
};

} // namespace kiyosi
