#pragma once

#include <kiyosi/instruments/range_accrual.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Exact sum of BSM marginal range probabilities, with deterministic rate/dividend curves.
/// Current observations use spot; earlier observations require explicit history.
/// Pre-effective valuations are valid. The engine is stateless and concurrent const calls are safe.
/// Allocation and internal failures propagate according to Result; calls are synchronous without cancellation.
class KIYOSI_EXPORT AnalyticRangeAccrualEngine {
public:
    [[nodiscard]] Result<double> price(const RangeAccrualOption&, const PricingContext&) const;
    /// Numerical Greeks use exact prices. At expiry all Greeks are unavailable;
    /// time stencils touching observations and spot stencils crossing current bounds are unavailable.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const RangeAccrualOption&, const PricingContext&,
                                                          GreeksRequest, NumericalShiftSettings = {}) const;
};

} // namespace kiyosi
