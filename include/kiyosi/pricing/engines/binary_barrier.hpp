#pragma once

#include <kiyosi/instruments/binary_barrier.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {
class KIYOSI_EXPORT AnalyticBinaryBarrierEngine {
public:
    [[nodiscard]] result<PricingResult> price(const BinaryBarrierOption&, const PricingContext&) const;
};
} // namespace kiyosi
