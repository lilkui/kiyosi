#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Closed-form Reiner-Rubinstein barrier valuation with a BGK shift for scheduled monitoring.
/// Untouched contracts with future scheduled observations require a final observation at expiry;
/// shorter monitoring windows return unsupported_operation. Resolved settlements remain supported.
class KIYOSI_EXPORT AnalyticBarrierEngine {
public:
    /// Prices a barrier option analytically.
    /// @return Price, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<double> price(const BarrierOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

private:
    [[nodiscard]] Result<PricingResult> price_native(const BarrierOption& option, const PricingContext& context) const;
};

} // namespace kiyosi
