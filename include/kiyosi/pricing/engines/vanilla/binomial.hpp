#pragma once

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/binomial.hpp>

namespace kiyosi {

namespace detail {
inline Result<double> binomial_time_step(const PricingContext& context, Date effective_date,
                                         Date expiry_date, BinomialSettings settings)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), effective_date, expiry_date);
    if (!valid) return std::unexpected(valid.error());
    if (settings.step_count <= 0 || settings.step_count > 1'000'000)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "binomial step count must be between 1 and 1000000"});
    return actual_365_fixed_year_fraction(context.valuation_time(), expiry_date) / static_cast<double>(settings.step_count);
}
} // namespace detail

/// Cox-Ross-Rubinstein binomial-tree engine for vanilla European and American options.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks use numerical differences.
/// Before expiry, volatility must be at least abs(r - q) * sqrt(T / step_count).
class KIYOSI_EXPORT CoxRossRubinsteinVanillaEngine {
public:
    /// Creates an engine with aggregate binomial settings.
    explicit CoxRossRubinsteinVanillaEngine(BinomialSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with an explicit tree step count.
    explicit CoxRossRubinsteinVanillaEngine(int step_count) : settings_{step_count} {}

    /// Prices a European or American vanilla option.
    /// @return Price, or a contract, context, or settings error.
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
    [[nodiscard]] Result<PricingResult> price_native(const EuropeanOption&, const PricingContext&, GreeksRequest) const;
    [[nodiscard]] Result<PricingResult> price_native(const AmericanOption&, const PricingContext&, GreeksRequest) const;
    BinomialSettings settings_;
};

} // namespace kiyosi
