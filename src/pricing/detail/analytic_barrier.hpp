#pragma once

#include <cmath>

#include <kiyosi/instruments/barrier/terms.hpp>

#include "math.hpp"

namespace kiyosi::detail {

// Callers validate schedules and resolve touched or completed contracts first.
[[nodiscard]] inline Result<double> analytic_barrier_log_shift(
    const BarrierTerms& terms, double spot, double volatility)
{
    if (terms.is_continuous()) return 0.0;
    // Broadie-Glasserman-Kou shift constant, -zeta(1/2)/sqrt(2*pi).
    constexpr double bgk_beta = 0.5825971579390107;
    const double shift = (terms.is_up() ? 1.0 : -1.0) * bgk_beta * volatility *
                         std::sqrt(terms.mean_observation_year_fraction());
    if (terms.observation_dates().back() != terms.expiry_date())
        return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                     "analytic barrier monitoring must end at expiry"});
    const double log_ratio = log_price_ratio(terms.barrier_level(), spot) + shift;
    // Preserve exact boundary comparisons when the shifted level is representable.
    const double barrier = terms.barrier_level() * std::exp(shift);
    const bool breached = std::isfinite(barrier) && barrier > 0.0
                              ? (terms.is_up() ? spot >= barrier : spot <= barrier)
                              : (terms.is_up() ? log_ratio <= 0.0 : log_ratio >= 0.0);
    if (breached)
        return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                     "spot must lie on the unbreached side of the shifted analytic barrier"});
    return shift;
}

} // namespace kiyosi::detail
