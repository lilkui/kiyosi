#pragma once

#include <algorithm>
#include <concepts>

#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>

namespace kiyosi::detail {

template <typename Option>
concept BarrierProduct = std::same_as<Option, BarrierOption> ||
                         std::same_as<Option, BinaryBarrierOption> || std::same_as<Option, TouchOption>;

template <BarrierProduct Option>
bool at_spot_discontinuity(const Option& option, const PricingContext& context, double radius = 0.0)
{
    const double spot = context.spot_price();
    return option.barrier_terms().touch_state() != BarrierTouchState::touched &&
           option.barrier_terms().is_monitored_at(context.valuation_time()) &&
           spot - radius <= option.barrier_level() && option.barrier_level() <= spot + radius;
}

template <BarrierProduct Option>
bool all_greeks_unavailable(const Option& option, const PricingContext& context)
{
    return at_spot_discontinuity(option, context);
}

template <BarrierProduct Option>
Result<bool> time_stencil_available(const Option& option, const PricingContext&, Timestamp before, Timestamp after)
{
    const auto& terms = option.barrier_terms();
    if (!terms.was_touched_before(before) || !terms.was_touched_before(after)) return false;
    return terms.is_continuous() || terms.touch_state() == BarrierTouchState::touched ||
           std::none_of(terms.observation_dates().begin(), terms.observation_dates().end(), [&](Date date) {
               const Timestamp event = start_of_day(date);
               return before <= event && event <= after;
           });
}

template <BarrierProduct Option>
bool volatility_affects_price(const Option& option, const PricingContext& context)
{
    const auto& terms = option.barrier_terms();
    const bool touched = *terms.was_touched_before(context.valuation_time()) ||
                         (terms.is_monitored_at(context.valuation_time()) && terms.is_breached_by(context.spot_price()));
    const bool monitoring_finished = !terms.is_continuous() &&
                                     start_of_day(terms.observation_dates().back()) <= context.valuation_time();
    if constexpr (std::same_as<Option, TouchOption>) {
        return !touched && !monitoring_finished;
    } else {
        if ((touched && !terms.is_knock_in()) || (!touched && monitoring_finished && terms.is_knock_in())) return false;
        const bool monitors_expiry = terms.is_continuous() || terms.observation_dates().back() == option.expiry_date();
        const bool payoff_excluded = !terms.is_knock_in() && monitors_expiry &&
                                     (terms.is_up() ? option.option_type() == OptionType::call && option.strike() >= terms.barrier_level()
                                                    : option.option_type() == OptionType::put && option.strike() <= terms.barrier_level());
        if constexpr (std::same_as<Option, BarrierOption>) return !payoff_excluded || option.rebate() != 0.0;
        else return !payoff_excluded;
    }
}

} // namespace kiyosi::detail
