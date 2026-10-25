#pragma once

#include <cmath>
#include <optional>

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/instruments/barrier/double.hpp>
#include <kiyosi/instruments/barrier/double_touch.hpp>

#include "black_scholes.hpp"

namespace kiyosi::detail {

inline Result<double> discounted_cash(double amount, const PricingContext& context, Date expiry)
{
    if (amount == 0.0) return 0.0;
    const auto discount = context.model_parameters().risk_free_curve().log_factor(context.valuation_time(), start_of_day(expiry));
    if (!discount) return std::unexpected(discount.error());
    return checked_price(scaled_exponential(amount, *discount));
}

// A terminal in-the-money fixing necessarily touches this barrier when expiry is monitored.
inline bool terminal_payoff_requires_barrier_touch(const BarrierTerms& terms, OptionType type, double strike)
{
    const bool monitors_expiry = terms.is_continuous() || terms.observation_dates().back() == terms.expiry_date();
    return monitors_expiry && (terms.is_up() ? type == OptionType::call && strike >= terms.barrier_level()
                                             : type == OptionType::put && strike <= terms.barrier_level());
}

inline bool terminal_payoff_requires_barrier_touch(const DoubleBarrierTerms& terms, OptionType type, double strike)
{
    const bool monitors_expiry = terms.is_continuous() || terms.observation_dates().back() == terms.expiry_date();
    return monitors_expiry && (type == OptionType::call ? strike >= terms.upper_barrier() : strike <= terms.lower_barrier());
}

// Callers validate instrument life and scheduled observation dates before resolving history.
inline std::optional<Result<double>> resolved_barrier_price(
    const DoubleTouchOption& option, const PricingContext& context)
{
    const auto state = barrier_monitoring_state(option.barrier_terms(), context.valuation_time(), context.spot_price());
    if (!state) return std::unexpected(state.error());
    if (!state->touched && !state->finished) return std::nullopt;
    if (state->touched != option.is_one_touch()) return 0.0;
    if (option.settlement_timing() == SettlementTiming::at_hit)
        return state->touched_before ? 0.0 : option.payout();
    return discounted_cash(option.payout(), context, option.expiry_date());
}

template <typename Option>
inline std::optional<Result<double>> resolved_barrier_price(
    const Option& option, const PricingContext& context)
{
    const auto& terms = option.barrier_terms();
    const auto state = barrier_monitoring_state(terms, context.valuation_time(), context.spot_price());
    if (!state) return std::unexpected(state.error());
    if (state->touched && !terms.is_knock_in())
        return option.rebate_timing() == RebateTiming::at_hit
                   ? checked_price(state->touched_before ? 0.0 : option.rebate())
                   : discounted_cash(option.rebate(), context, option.expiry_date());
    if (!state->touched && state->finished && terms.is_knock_in())
        return discounted_cash(option.rebate(), context, option.expiry_date());
    const bool fixed_payoff = option.rebate() == 0.0 && terminal_payoff_requires_barrier_touch(terms, option.option_type(), option.strike());
    if (fixed_payoff && !terms.is_knock_in()) return 0.0;
    if (state->touched || state->finished || fixed_payoff) {
        const auto vanilla = price_at_volatility(
            *make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()),
            context, context.model_parameters().volatility(), GreeksRequest{});
        if (!vanilla) return std::unexpected(vanilla.error());
        return checked_price(vanilla->price());
    }
    return std::nullopt;
}

} // namespace kiyosi::detail
