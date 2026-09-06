#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/barrier.hpp>
#include <kiyosi/pricing/engines/finite_difference.hpp>

namespace kiyosi {

class AnalyticBarrierEngine {
public:
    [[nodiscard]] result<PricingResult> price(
        const BarrierOption&, const PricingContext&) const;
};
class FiniteDifferenceBarrierEngine {
public:
    explicit FiniteDifferenceBarrierEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceBarrierEngine(int asset_steps, int time_steps,
                                  finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}
    [[nodiscard]] result<PricingResult> price(const BarrierOption& option, const PricingContext& context) const
    {
        return AnalyticBarrierEngine{}.price(option, context);
    }

private:
    FiniteDifferenceSettings settings_;
};
using FdBarrierEngine = FiniteDifferenceBarrierEngine;

} // namespace kiyosi
