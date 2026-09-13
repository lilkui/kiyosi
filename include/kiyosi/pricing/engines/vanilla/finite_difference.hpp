#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference European engine for vanilla options.
class KIYOSI_EXPORT FiniteDifferenceEuropeanEngine {
public:
    explicit FiniteDifferenceEuropeanEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceEuropeanEngine(int asset_steps, int time_steps,
                                   finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}

    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption& option, const PricingContext& context) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

/// Uniform-grid finite-difference American engine with early exercise at every time layer.
class KIYOSI_EXPORT FiniteDifferenceAmericanEngine {
public:
    explicit FiniteDifferenceAmericanEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceAmericanEngine(int asset_steps, int time_steps,
                                   finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}

    [[nodiscard]] result<PricingResult> price(
        const AmericanOption& option, const PricingContext& context) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
