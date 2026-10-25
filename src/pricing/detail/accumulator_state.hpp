#pragma once

#include <optional>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/accumulator_program.hpp>

namespace kiyosi::detail {

struct AccumulatorInitialState {
    double quantity{};
    std::optional<double> settlement;
};

inline AccumulatorInitialState accumulator_initial_state(
    const Accumulator& option, const PricingContext& context)
{
    const Timestamp valuation = context.valuation_time();
    const double value = context.spot_price();
    double quantity = option.accumulated_quantity();
    if (quantity == 0.0 && !accumulator_accrues(option)) return {quantity, 0.0};
    if (valuation == start_of_day(date_of(valuation)) &&
        context.calendar().is_trading_day(date_of(valuation))) {
        if (accumulator_knocked_out(value, option.knock_out_level()))
            return {quantity, quantity * (value - option.strike())};
        quantity += accumulator_purchase_quantity(value, option.strike(), option.daily_quantity(), option.acceleration_factor());
    }
    if (valuation == option.expiry_date()) return {quantity, quantity * (value - option.strike())};
    // Stopping a martingale preserves the expected value of a fixed position.
    if (!accumulator_accrues(option) && context.model_parameters().has_flat_rates() && (*context.model_parameters().risk_free_rate()) == 0.0 &&
        (*context.model_parameters().dividend_yield()) == 0.0)
        return {quantity, quantity * (value - option.strike())};
    return {quantity, std::nullopt};
}

} // namespace kiyosi::detail
