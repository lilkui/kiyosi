#pragma once

#include <kiyosi/instruments/barrier/double.hpp>
#include <kiyosi/market/context.hpp>

namespace kiyosi::detail {

// Continuous first-exit cash value, shared by analytical and Monte Carlo engines.
Result<double> double_barrier_hit_value(const DoubleBarrierOption& option, const PricingContext& context);

} // namespace kiyosi::detail
