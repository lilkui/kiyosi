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
    const auto state = barrier_monitoring_state(terms, context.valuation_time(), context.spot_price());
    if (!state) return std::unexpected(state.error());
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    if (state->touched && !terms.is_knock_in())
        return checked_price(option.rebate_timing() == RebateTiming::at_hit
                                 ? (state->touched_before ? 0.0 : option.rebate())
                                 : scaled_exponential(option.rebate(), -context.model_parameters().risk_free_rate() * time));
    if (!state->touched && state->finished && terms.is_knock_in())
        return checked_price(scaled_exponential(option.rebate(), -context.model_parameters().risk_free_rate() * time));
    if (state->touched || state->finished) {
        const auto vanilla = price_at_volatility(
            *make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()),
            context, context.model_parameters().volatility(), GreeksRequest{});
        if (!vanilla) return std::unexpected(vanilla.error());
        return checked_price(vanilla->price());
    }
    return std::nullopt;
}

} // namespace kiyosi::detail
