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

class AnalyticEuropeanEngine {
public:
    static constexpr risk_measure_set supported_risk_measures = all_risk_measures;

    /// Returns intrinsic value and zero Greeks when valued at expiry.
    [[nodiscard]] result<PricingResult> price(const EuropeanOption& option, const PricingContext& context) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption& option, const PricingContext& context, PricingRequest request) const;

    [[nodiscard]] result<double> implied_volatility(
        const EuropeanOption& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const;
};

} // namespace kiyosi

