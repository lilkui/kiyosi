#pragma once

#include <kiyosi/instruments/barrier.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

class KIYOSI_EXPORT AnalyticBarrierEngine {
public:
    [[nodiscard]] result<PricingResult> price(
        const BarrierOption&, const PricingContext&) const;
};

} // namespace kiyosi
