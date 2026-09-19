#pragma once

#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form Rubinstein-Reiner binary barrier and touch valuation.
class KIYOSI_EXPORT AnalyticBinaryBarrierEngine {
public:
    [[nodiscard]] Result<PricingResult> price(const BinaryBarrierOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price(const TouchOption&, const PricingContext&) const;
};

} // namespace kiyosi
