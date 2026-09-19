#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceBarrierEngine {
public:
    explicit FiniteDifferenceBarrierEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceBarrierEngine(int asset_step_count, int time_step_count,
                                  FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    [[nodiscard]] Result<PricingResult> price(const BarrierOption& option, const PricingContext& context) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
