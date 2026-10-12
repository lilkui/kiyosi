#pragma once

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/market/context.hpp>

namespace kiyosi::detail {

template <typename Averaging>
Result<bool> time_stencil_available(const AveragePriceOption<Averaging>& option, const PricingContext&,
                                    Timestamp before, Timestamp after)
{
    const Timestamp averaging_start = start_of_day(option.averaging_start_date());
    return option.realized_average() == 0.0 ? after <= averaging_start : before > averaging_start;
}

inline bool volatility_affects_price(const ArithmeticAveragePriceOption& option, const PricingContext& context)
{
    return arithmetic_average_adjusted_strike(option, context.valuation_time()) > 0.0;
}

} // namespace kiyosi::detail
