#pragma once

#include <chrono>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/accumulator_program.hpp>

namespace kiyosi::detail {

inline bool at_spot_discontinuity(const Accumulator& option, const PricingContext& context, double radius = 0.0)
{
    if (context.valuation_time() != start_of_day(context.valuation_date()) ||
        !context.calendar().is_trading_day(context.valuation_date())) return false;
    const double spot = context.spot_price();
    const auto crosses = [&](double level) { return spot - radius <= level && level <= spot + radius; };
    return crosses(option.knock_out_level()) ||
           (!accumulator_knocked_out(spot, option.knock_out_level()) && crosses(option.strike()) && option.acceleration_factor() != 1.0);
}

inline Result<bool> time_stencil_available(const Accumulator&, const PricingContext& context,
                                           Timestamp before, Timestamp after)
{
    for (Date date = date_of(before); date <= date_of(after); date += std::chrono::days{1}) {
        const Timestamp event = start_of_day(date);
        if (before <= event && event <= after && context.calendar().is_trading_day(date)) return false;
    }
    return true;
}

inline bool volatility_affects_price(const Accumulator& option, const PricingContext& context)
{
    const bool knocked_out_now = context.valuation_time() == start_of_day(context.valuation_date()) &&
                                 context.calendar().is_trading_day(context.valuation_date()) &&
                                 accumulator_knocked_out(context.spot_price(), option.knock_out_level());
    // With zero rates and no new accrual, stopping an accumulated forward preserves its expectation.
    const bool exposed_quantity = option.accumulated_quantity() != 0.0 &&
                                  (context.model_parameters().risk_free_rate() != 0.0 ||
                                   context.model_parameters().dividend_yield() != 0.0);
    return !knocked_out_now && (accumulator_accrues(option) || exposed_quantity);
}

} // namespace kiyosi::detail
