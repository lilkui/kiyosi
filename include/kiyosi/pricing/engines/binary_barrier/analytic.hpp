#pragma once

#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form Rubinstein-Reiner binary barrier and touch valuation.
class KIYOSI_EXPORT AnalyticBinaryBarrierEngine {
public:
    /// Prices a strike-based binary barrier option.
    /// @return Pricing measures, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<PricingResult> price(const BinaryBarrierOption&, const PricingContext&) const;
    /// Prices a one-touch or no-touch option.
    /// @return Pricing measures, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<PricingResult> price(const TouchOption&, const PricingContext&) const;
};

} // namespace kiyosi
