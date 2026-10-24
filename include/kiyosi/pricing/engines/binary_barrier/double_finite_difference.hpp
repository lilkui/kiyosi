#pragma once

#include <type_traits>

#include <kiyosi/instruments/barrier/double_touch.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceDoubleTouchEngine;
[[nodiscard]] Result<FiniteDifferenceDoubleTouchEngine> make_finite_difference_double_touch_engine(FiniteDifferenceSettings);

/// BSM finite differences for fixed-cash double one-touch and double no-touch.
/// Continuous grids fit both physical barriers; asset steps span the live interval.
/// Scheduled grids span zero to the configured upper boundary and project both
/// barriers simultaneously, retaining continuation outside them between observations.
/// An explicit upper boundary must exceed spot and both barriers, even for continuous
/// monitoring where it does not set the live grid. Refine space/time and scheduled
/// domain separately, especially near barriers, narrow bands and fixing dates.
/// At a currently observed barrier, or at expiry, only price is available.
/// Calls on shared/distinct engines and immutable inputs are safe concurrently;
/// concurrent assignment is not safe. All work buffers are request-local.
/// Allocation failures propagate as std::bad_alloc. Synchronous, without cancellation.
class KIYOSI_EXPORT FiniteDifferenceDoubleTouchEngine {
public:
    FiniteDifferenceDoubleTouchEngine() = default;
    [[nodiscard]] Result<double> price(const DoubleTouchOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const DoubleTouchOption&, const PricingContext&,
                                                          GreeksRequest, NumericalShiftSettings = {}) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    explicit FiniteDifferenceDoubleTouchEngine(FiniteDifferenceSettings settings) : settings_(settings) {}
    friend Result<FiniteDifferenceDoubleTouchEngine> make_finite_difference_double_touch_engine(FiniteDifferenceSettings);
    friend Result<FiniteDifferenceDoubleTouchEngine> make_engine(FiniteDifferenceSettings settings, std::type_identity<FiniteDifferenceDoubleTouchEngine>)
    {
        return make_finite_difference_double_touch_engine(settings);
    }
    FiniteDifferenceSettings settings_;
};

[[nodiscard]] inline Result<FiniteDifferenceDoubleTouchEngine> make_finite_difference_double_touch_engine(FiniteDifferenceSettings settings)
{
    const auto valid = validate_finite_difference_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return FiniteDifferenceDoubleTouchEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_finite_difference_engine<FiniteDifferenceDoubleTouchEngine, Option> = true;
}

} // namespace kiyosi
