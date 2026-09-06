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
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context,
        PricingRequest request = {}) const
    {
        return price_impl(option, context, request);
    }

    [[nodiscard]] result<double> implied_volatility(
        const EuropeanOption& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const;

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const EuropeanOption&, const PricingContext&, PricingRequest) const;
};

} // namespace kiyosi
