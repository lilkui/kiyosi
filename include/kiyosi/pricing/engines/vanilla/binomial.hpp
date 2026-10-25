#pragma once

#include <kiyosi/pricing/detail/binomial.hpp>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/detail/instrument_analytics.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/binomial.hpp>

namespace kiyosi {

class KIYOSI_EXPORT CoxRossRubinsteinVanillaEngine;
[[nodiscard]] Result<CoxRossRubinsteinVanillaEngine> make_cox_ross_rubinstein_vanilla_engine(BinomialSettings settings);

/// Cox-Ross-Rubinstein binomial-tree engine for vanilla European and American options.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks use numerical differences.
/// Before expiry, volatility must be at least abs(r - q) * sqrt(T / step_count).
class KIYOSI_EXPORT CoxRossRubinsteinVanillaEngine {
public:
    /// Creates an engine with valid default settings.
    CoxRossRubinsteinVanillaEngine() = default;

    /// Prices a European or American vanilla option.
    /// @return Price, or a contract, context, or numerical error.
    [[nodiscard]] Result<double> price(const EuropeanOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AmericanOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    BinomialSettings settings() const noexcept { return settings_; }

private:
    explicit CoxRossRubinsteinVanillaEngine(BinomialSettings settings) : settings_(settings) {}
    friend Result<CoxRossRubinsteinVanillaEngine> make_cox_ross_rubinstein_vanilla_engine(BinomialSettings);

    BinomialSettings settings_;
};

/// Creates an engine after validating its configuration.
/// @return The engine, or an `invalid_parameter` error.
[[nodiscard]] inline Result<CoxRossRubinsteinVanillaEngine> make_cox_ross_rubinstein_vanilla_engine(BinomialSettings settings)
{
    const auto valid = validate_binomial_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return CoxRossRubinsteinVanillaEngine{settings};
}

namespace detail {
template <typename Option>
Result<ImpliedVolatilitySettings> prepare_implied_volatility(
    const CoxRossRubinsteinVanillaEngine& engine, const Option& option, const PricingContext& context,
    ImpliedVolatilitySettings settings)
{
    return prepare_binomial_implied_volatility(engine.settings(), option, context, settings);
}
} // namespace detail

} // namespace kiyosi
