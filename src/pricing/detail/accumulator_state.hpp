#pragma once

#include <optional>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>

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
    if (quantity == 0.0 && option.daily_quantity() == 0.0) return {quantity, 0.0};
    if (valuation == start_of_day(date_of(valuation)) &&
        context.calendar().is_trading_day(date_of(valuation))) {
        if (value >= option.knock_out_level())
            return {quantity, quantity * (value - option.strike())};
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                            : option.daily_quantity();
    }
    if (valuation == option.expiry_date()) return {quantity, quantity * (value - option.strike())};
    return {quantity, std::nullopt};
}

} // namespace kiyosi::detail
