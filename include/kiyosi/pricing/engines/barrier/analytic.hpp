#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form Reiner-Rubinstein barrier valuation with a BGK shift for scheduled monitoring.
class KIYOSI_EXPORT AnalyticBarrierEngine {
public:
    /// Prices a barrier option analytically.
    /// @return Pricing measures, or a contract, context, or unsupported-operation error.
    [[nodiscard]] Result<PricingResult> price(const BarrierOption&, const PricingContext&) const;
};

} // namespace kiyosi
