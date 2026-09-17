#pragma once

#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form Rubinstein-Reiner binary barrier and touch valuation.
class KIYOSI_EXPORT AnalyticBinaryBarrierEngine {
public:
    [[nodiscard]] result<PricingResult> price(const BinaryBarrierOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const TouchOption&, const PricingContext&) const;
};

} // namespace kiyosi
