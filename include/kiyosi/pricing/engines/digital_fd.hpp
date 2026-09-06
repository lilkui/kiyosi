#pragma once

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/engines/finite_difference.hpp>

namespace kiyosi {
class FiniteDifferenceDigitalEngine {
public:
    explicit FiniteDifferenceDigitalEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceDigitalEngine(int asset_steps, int time_steps,
                                  finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}
    [[nodiscard]] result<PricingResult> price(const EuropeanCashOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanAssetOrNothingOption&, const PricingContext&) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};
class IntegralDigitalEngine {
public:
    [[nodiscard]] result<PricingResult> price(const EuropeanCashOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanAssetOrNothingOption&, const PricingContext&) const;
};
using FdDigitalEngine = FiniteDifferenceDigitalEngine;
} // namespace kiyosi
