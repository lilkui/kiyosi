#pragma once

#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Closed-form Rubinstein-Reiner binary barrier and touch valuation.
/// Scheduled monitoring uses a BGK barrier shift. Untouched contracts with future observations
/// require a final observation at expiry; shorter windows return unsupported_operation.
/// Resolved settlements remain supported.
class KIYOSI_EXPORT AnalyticBinaryBarrierEngine {
public:
    /// Prices a strike-based binary barrier option.
    /// @return Price, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<double> price(const BinaryBarrierOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BinaryBarrierOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Prices a one-touch or no-touch option.
    /// @return Price, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<double> price(const TouchOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const TouchOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
};

} // namespace kiyosi
