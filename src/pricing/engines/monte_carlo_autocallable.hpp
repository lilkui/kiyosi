#pragma once

#include <kiyosi/pricing/detail/autocallable_program.hpp>

#include "monte_carlo_discount.hpp"

#if defined(__CUDACC__)
#define KIYOSI_AUTOCALLABLE_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_AUTOCALLABLE_HOST_DEVICE
#endif

namespace kiyosi::detail {

// Returns true when this observation settles the path, writing its discounted payoff.
KIYOSI_AUTOCALLABLE_HOST_DEVICE inline bool observe_autocallable_path(
    const AutocallableProgram& program, const AutocallableEvent& event,
    const MonteCarloDiscount& discount, double spot, AutocallablePathState& state, double& payoff)
{
    state.knocked_in = program_knocked_in(program, spot, state.knocked_in, false);
    if (!event.active) return false;
    const double coupon = program_observation_coupon(event, spot);
    if (spot >= event.knock_out_level) {
        payoff = discount.apply(program.principal_ratio + coupon) + state.coupons;
        return true;
    }
    if (program.carries_observation_coupon)
        state.coupons += discount.apply(coupon);
    return false;
}

KIYOSI_AUTOCALLABLE_HOST_DEVICE inline double settle_autocallable_path(
    const AutocallableProgram& program, const MonteCarloDiscount& discount,
    double spot, AutocallablePathState state)
{
    state.knocked_in = program_knocked_in(program, spot, state.knocked_in, true);
    return state.coupons + discount.apply(program_terminal_settlement(program, spot, state.knocked_in));
}

} // namespace kiyosi::detail

#undef KIYOSI_AUTOCALLABLE_HOST_DEVICE
