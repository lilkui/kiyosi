#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

struct ImpliedVolatilitySettings {
    double lower_bound = 0.0001;
    double upper_bound = 4.0;
    double tolerance = 1e-8;
    int max_iterations = 100;
};

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
