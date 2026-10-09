#pragma once

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Closed-form cash-or-nothing and asset-or-nothing valuation with analytic delta and gamma.
class KIYOSI_EXPORT AnalyticDigitalEngine {
public:
    /// Prices a European cash-or-nothing or asset-or-nothing option.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const CashOrNothingOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AssetOrNothingOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const CashOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AssetOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

private:
    [[nodiscard]] Result<PricingResult> price_native(const CashOrNothingOption& option, const PricingContext& context, GreeksRequest output) const;
    [[nodiscard]] Result<PricingResult> price_native(const AssetOrNothingOption& option, const PricingContext& context, GreeksRequest output) const;

    [[nodiscard]] Result<PricingResult> price_impl(
        OptionType, double, double, bool, Date, Date, const PricingContext&, GreeksRequest) const;
};

} // namespace kiyosi
