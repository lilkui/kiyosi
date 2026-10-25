#pragma once

#include <type_traits>

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceBarrierEngine;
[[nodiscard]] Result<FiniteDifferenceBarrierEngine> make_finite_difference_barrier_engine(FiniteDifferenceSettings settings);

/// Uniform-grid finite-difference engine for barrier options.
/// Continuous barriers are exact endpoints of the live asset-price domain.
/// Crank-Nicolson uses two implicit Euler half-steps per interval for up to two
/// initial intervals. Scheduled observations locally average the barrier event and
/// restart this damping for the barrier layer; the knock-in vanilla reference keeps
/// its original time-marching schedule on the same space and time grids.
/// Scheduled observations are applied only at their original time-grid endpoints.
/// Current observations and expiry settlement use the actual spot without averaging.
class KIYOSI_EXPORT FiniteDifferenceBarrierEngine {
public:
    /// Creates an engine with valid default settings.
    FiniteDifferenceBarrierEngine() = default;

    /// Prices a barrier option by finite differences.
    /// @return Price, or a contract, context, or numerical error.
    [[nodiscard]] Result<double> price(const BarrierOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// Delta, gamma, and speed reuse the solved layer; vanna/zomma and charm/color difference native delta/gamma shifted in volatility and time.
    /// Other measures use numerical price differences.
    /// Scheduled observation dates are validated once per request; shifted valuations retain their other checks.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    explicit FiniteDifferenceBarrierEngine(FiniteDifferenceSettings settings) : settings_(settings) {}
    friend Result<FiniteDifferenceBarrierEngine> make_finite_difference_barrier_engine(FiniteDifferenceSettings);
    friend Result<FiniteDifferenceBarrierEngine> make_engine(FiniteDifferenceSettings settings, std::type_identity<FiniteDifferenceBarrierEngine>)
    {
        return make_finite_difference_barrier_engine(settings);
    }

    FiniteDifferenceSettings settings_;
};

/// Creates an engine after validating its configuration.
/// @return The engine, or an `invalid_parameter` error.
[[nodiscard]] inline Result<FiniteDifferenceBarrierEngine> make_finite_difference_barrier_engine(FiniteDifferenceSettings settings)
{
    const auto valid = validate_finite_difference_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return FiniteDifferenceBarrierEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_finite_difference_engine<FiniteDifferenceBarrierEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
