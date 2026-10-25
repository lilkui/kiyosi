#pragma once

#include <kiyosi/instruments/barrier/double.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// European double-barrier valuation under constant-coefficient BSM dynamics.
/// Continuous monitoring uses an adaptively converged absorbing-density series.
/// Unresolved scheduled contracts return unsupported_operation; resolved settlements work.
/// Both rebate timings are supported, including negative rates. Numerical convergence or
/// representability failures return invalid_result; there is no fallback to another engine.
/// Requested Greeks use numerical shifts and the common boundary guards.
/// All workspace is request-local. Concurrent const calls on shared or distinct instances
/// are safe without concurrent assignment. Runtime exceptions propagate; no cancellation API.
class KIYOSI_EXPORT AnalyticDoubleBarrierEngine {
public:
    [[nodiscard]] Result<double> price(const DoubleBarrierOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const DoubleBarrierOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
};

} // namespace kiyosi
