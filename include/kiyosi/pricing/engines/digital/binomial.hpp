#pragma once

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/binomial.hpp>
#include <kiyosi/pricing/detail/instrument_analytics.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT CoxRossRubinsteinDigitalEngine;
[[nodiscard]] Result<CoxRossRubinsteinDigitalEngine> make_cox_ross_rubinstein_digital_engine(BinomialSettings settings);

/// Direct CRR terminal-payoff valuation of European cash-or-nothing and asset-or-nothing options.
/// Price convergence can oscillate with strike alignment. Delta and gamma are node estimates;
/// other Greeks are unavailable, and gamma requires at least two steps. Shifts are validated but unused.
/// Before expiry, volatility must be at least abs(r - q) * sqrt(T / step_count).
/// Concurrent const calls on shared or distinct engines are safe without concurrent mutation.
class KIYOSI_EXPORT CoxRossRubinsteinDigitalEngine {
public:
    CoxRossRubinsteinDigitalEngine() = default;

    [[nodiscard]] Result<double> price(const CashOrNothingOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AssetOrNothingOption& option, const PricingContext& context) const;

    /// Returns price and the requested supported Greeks; all Greeks are unavailable at expiry.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const CashOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AssetOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    [[nodiscard]] BinomialSettings settings() const noexcept { return settings_; }

private:
    explicit CoxRossRubinsteinDigitalEngine(BinomialSettings settings) : settings_(settings) {}
    friend Result<CoxRossRubinsteinDigitalEngine> make_cox_ross_rubinstein_digital_engine(BinomialSettings);
    BinomialSettings settings_;
};

/// Validates the configuration; rejects invalid settings with invalid_parameter.
[[nodiscard]] inline Result<CoxRossRubinsteinDigitalEngine> make_cox_ross_rubinstein_digital_engine(BinomialSettings settings)
{
    const auto valid = validate_binomial_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return CoxRossRubinsteinDigitalEngine{settings};
}

namespace detail {
template <typename Option>
Result<ImpliedVolatilitySettings> prepare_implied_volatility(
    const CoxRossRubinsteinDigitalEngine& engine, const Option& option, const PricingContext& context,
    ImpliedVolatilitySettings settings)
{
    return prepare_binomial_implied_volatility(engine.settings(), option, context, settings);
}
} // namespace detail

} // namespace kiyosi
