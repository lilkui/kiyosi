#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form Reiner-Rubinstein barrier valuation with a BGK shift for scheduled monitoring.
class KIYOSI_EXPORT AnalyticBarrierEngine {
public:
    [[nodiscard]] result<PricingResult> price(const BarrierOption&, const PricingContext&) const;
};

} // namespace kiyosi
