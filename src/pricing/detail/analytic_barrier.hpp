#pragma once

#include <cmath>

#include <kiyosi/instruments/barrier/terms.hpp>

namespace kiyosi::detail {

// Callers validate schedules and resolve touched or completed contracts first.
[[nodiscard]] inline Result<double> analytic_barrier_level(
    const BarrierTerms& terms, double spot, double volatility)
{
    if (terms.is_continuous()) return terms.barrier_level();
    // Broadie-Glasserman-Kou shift constant, -zeta(1/2)/sqrt(2*pi).
    constexpr double bgk_beta = 0.5825971579390107;
    const double barrier = terms.barrier_level() *
                           std::exp((terms.is_up() ? 1.0 : -1.0) * bgk_beta * volatility *
                                    std::sqrt(terms.mean_observation_year_fraction()));
    if (terms.observation_dates().back() != terms.expiry_date())
        return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                     "analytic barrier monitoring must end at expiry"});
    if (terms.is_up() ? spot >= barrier : spot <= barrier)
        return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                     "spot must lie on the unbreached side of the shifted analytic barrier"});
    return barrier;
}

} // namespace kiyosi::detail
