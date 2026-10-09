#pragma once

#include <cmath>
#include <optional>

#include <kiyosi/instruments/barrier/option.hpp>

#include "black_scholes.hpp"

namespace kiyosi::detail {

// Callers validate instrument life and scheduled observation dates before resolving history.
inline std::optional<Result<double>> resolved_barrier_price(
    const BarrierOption& option, const PricingContext& context)
{
    const auto& terms = option.barrier_terms();
    const auto prior_touch = terms.was_touched_before(context.valuation_time());
    if (!prior_touch) return std::unexpected(prior_touch.error());
    const bool touched = *prior_touch ||
                         (terms.is_monitored_at(context.valuation_time()) &&
                          terms.is_breached_by(context.spot_price()));
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    if (touched && !terms.is_knock_in())
        return checked_price(option.rebate_timing() == RebateTiming::at_hit
                                 ? (*prior_touch ? 0.0 : option.rebate())
                                 : option.rebate() * std::exp(-context.model_parameters().risk_free_rate() * time));
    const bool monitoring_finished = !terms.is_continuous() &&
                                     start_of_day(terms.observation_dates().back()) <= context.valuation_time();
    if (!touched && monitoring_finished && terms.is_knock_in())
        return checked_price(option.rebate() * std::exp(-context.model_parameters().risk_free_rate() * time));
    if (touched || monitoring_finished) {
        const auto vanilla = price_at_volatility(
            *make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()),
            context, context.model_parameters().volatility(), GreeksRequest{});
        if (!vanilla) return std::unexpected(vanilla.error());
        return checked_price(vanilla->price());
    }
    return std::nullopt;
}

} // namespace kiyosi::detail
