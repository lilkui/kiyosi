#pragma once

#include <kiyosi/instruments/barrier.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/engines/settings/finite_difference.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceBarrierEngine {
public:
    explicit FiniteDifferenceBarrierEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceBarrierEngine(int asset_steps, int time_steps,
                                  finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}
    [[nodiscard]] result<PricingResult> price(const BarrierOption& option, const PricingContext& context) const;

private:
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
