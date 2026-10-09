#pragma once

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Simpson quadrature over the terminal lognormal density.
class KIYOSI_EXPORT QuadratureDigitalEngine {
public:
    /// Prices a cash-or-nothing option by numerical quadrature.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const CashOrNothingOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const CashOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Prices an asset-or-nothing option by numerical quadrature.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const AssetOrNothingOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AssetOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

private:
    [[nodiscard]] Result<PricingResult> price_native(const CashOrNothingOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<PricingResult> price_native(const AssetOrNothingOption& option, const PricingContext& context) const;
};

} // namespace kiyosi
