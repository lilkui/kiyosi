#pragma once

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Prices an autocallable note with the finite-difference implementation.
/// @return Price, or a contract, context, or settings error.
template <typename Note>
[[nodiscard]] KIYOSI_EXPORT Result<PricingResult> price_autocallable_finite_difference(
    const Note&, const PricingContext&, FiniteDifferenceSettings);

/// One- or two-layer backward induction, depending on whether the note has knock-in state, with
/// knock-out and coupon events anchored onto the time grid.
template <typename Note>
class KIYOSI_EXPORT FiniteDifferenceAutocallableEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceAutocallableEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceAutocallableEngine(int asset_step_count, int time_step_count,
                                       FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices an autocallable note by finite differences.
    /// @return Price, or a contract, context, or settings error.
    [[nodiscard]] Result<double> price(const Note& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const Note& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_native(const Note& option, const PricingContext& context) const
    {
        return price_autocallable_finite_difference(option, context, settings_);
    }
    FiniteDifferenceSettings settings_;
};

/// Finite-difference engine for Phoenix options.
using FiniteDifferencePhoenixEngine = FiniteDifferenceAutocallableEngine<PhoenixOption>;
/// Finite-difference engine for snowball options.
using FiniteDifferenceSnowballEngine = FiniteDifferenceAutocallableEngine<SnowballOption>;
/// Finite-difference engine for binary snowball options.
using FiniteDifferenceBinarySnowballEngine = FiniteDifferenceAutocallableEngine<BinarySnowballOption>;
/// Finite-difference engine for ternary snowball options.
using FiniteDifferenceTernarySnowballEngine = FiniteDifferenceAutocallableEngine<TernarySnowballOption>;

} // namespace kiyosi
