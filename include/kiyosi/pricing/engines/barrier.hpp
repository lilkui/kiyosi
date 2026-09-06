#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/barrier.hpp>

namespace kiyosi {

class AnalyticBarrierEngine {
public:
    [[nodiscard]] result<PricingResult> price(
        const BarrierOption&, const PricingContext&) const;
};

} // namespace kiyosi
