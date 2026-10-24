#pragma once

#include <type_traits>

#include <kiyosi/instruments/barrier/double.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceDoubleBarrierEngine;
[[nodiscard]] Result<FiniteDifferenceDoubleBarrierEngine> make_finite_difference_double_barrier_engine(FiniteDifferenceSettings);

/// Finite differences for fixed double barriers under the BSM model.
/// Continuous grids fit both physical boundaries exactly; scheduled grids span
/// zero to the configured upper boundary and project both events simultaneously.
/// Asset steps span the live interval for continuous monitoring. An explicit upper
/// boundary must exceed spot, strike and both barriers; it sets only scheduled domains.
/// CN damping and observation/Greek availability follow the single-barrier engine.
/// Grid refinement is required for accuracy, especially near barriers and in narrow bands.
/// All calls on shared/distinct engines and immutable inputs are safe concurrently;
/// concurrent assignment to any input/engine is not safe. Work buffers are request-local.
/// Allocation failures propagate as std::bad_alloc; calls are synchronous and do not cancel.
class KIYOSI_EXPORT FiniteDifferenceDoubleBarrierEngine {
public:
    FiniteDifferenceDoubleBarrierEngine() = default;
    [[nodiscard]] Result<double> price(const DoubleBarrierOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const DoubleBarrierOption&, const PricingContext&,
                                                          GreeksRequest, NumericalShiftSettings = {}) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    explicit FiniteDifferenceDoubleBarrierEngine(FiniteDifferenceSettings settings) : settings_(settings) {}
    friend Result<FiniteDifferenceDoubleBarrierEngine> make_finite_difference_double_barrier_engine(FiniteDifferenceSettings);
    friend Result<FiniteDifferenceDoubleBarrierEngine> make_engine(FiniteDifferenceSettings settings, std::type_identity<FiniteDifferenceDoubleBarrierEngine>)
    {
        return make_finite_difference_double_barrier_engine(settings);
    }
    FiniteDifferenceSettings settings_;
};

[[nodiscard]] inline Result<FiniteDifferenceDoubleBarrierEngine> make_finite_difference_double_barrier_engine(FiniteDifferenceSettings settings)
{
    const auto valid = validate_finite_difference_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return FiniteDifferenceDoubleBarrierEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_finite_difference_engine<FiniteDifferenceDoubleBarrierEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
