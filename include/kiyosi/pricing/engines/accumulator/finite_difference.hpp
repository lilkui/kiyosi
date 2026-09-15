#pragma once

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Marches the accrual value back as an affine function of the accumulated quantity, so a single
/// sweep prices every opening position.
class KIYOSI_EXPORT FiniteDifferenceAccumulatorEngine {
public:
    explicit FiniteDifferenceAccumulatorEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceAccumulatorEngine(int asset_steps, int time_steps,
                                      finite_difference_scheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_steps, time_steps, scheme} {}

    [[nodiscard]] result<PricingResult> price(const Accumulator&, const PricingContext&) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
