#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/binomial.hpp>

namespace kiyosi {

/// Cox-Ross-Rubinstein binomial-tree engine for vanilla European and American options.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks use numerical differences.
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
