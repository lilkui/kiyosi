#pragma once

#include <kiyosi/instruments/structured.hpp>
#include <kiyosi/market/context.hpp>
#include "common.hpp"
#include <algorithm>
#include <type_traits>
#include <vector>

namespace kiyosi::detail {

inline std::vector<date> trading_dates(const TradingCalendar& calendar, timestamp start, date end, bool include_start = false)
{
    std::vector<date> dates;
    for (auto value = include_start && start == start_of_day(date_of(start)) ? date_of(start) : date_of(start) + std::chrono::days{1}; value <= end; value += std::chrono::days{1})
        if (calendar.is_trading_day(value)) dates.push_back(value);
    return dates;
}

template <typename Option>
std::vector<std::size_t> observation_schedule(const Option& option, timestamp valuation)
{
    std::vector<std::size_t> schedule;
    if constexpr (requires { option.observation_dates(); }) {
        const auto& dates = option.observation_dates();
        for (std::size_t index = 0; index < dates.size(); ++index)
            if (dates[index] >= valuation) schedule.push_back(index);
    }
    return schedule;
}

template <typename Option>
double terminal_settlement(const Option& option, double spot, bool knocked_in)
{
    if constexpr (std::is_same_v<Option, PhoenixOption>) {
        const double loss = std::clamp(spot - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) /
                            option.initial_price();
        return option.principal_ratio() + (knocked_in ? loss : 0.0);
    } else if constexpr (std::is_same_v<Option, SnowballOption>) {
        const double loss = std::clamp(spot - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) /
                            option.initial_price();
        const double coupon = knocked_in ? loss : option.maturity_coupon_rate() * actual_365(option.effective(), option.expiry());
        return option.principal_ratio() + coupon;
    } else if constexpr (std::is_same_v<Option, TernarySnowballOption>) {
        const double rate = knocked_in ? option.minimal_coupon_rate() : option.maturity_coupon_rate();
        return option.principal_ratio() + rate * actual_365(option.effective(), option.expiry());
    } else {
        return option.principal_ratio() + option.maturity_coupon_rate() * actual_365(option.effective(), option.expiry());
    }
}

template <typename Option>
double observation_coupon(const Option& option, std::size_t index, double spot)
{
    if constexpr (std::is_same_v<Option, PhoenixOption>)
        return spot >= option.coupon_barriers()[index] ? option.initial_price() * option.coupon_rate() : 0.0;
    else
        return option.knock_out_coupon_rates()[index] * actual_365(option.effective(), option.observation_dates()[index]);
}

template <typename Option>
bool is_knocked_in(const Option& option, double spot, bool knocked_in, bool expiry)
{
    if constexpr (requires { option.knock_in_price(); }) {
        if (option.knock_in_frequency() == observation_frequency::daily || expiry)
            return knocked_in || spot < option.knock_in_price();
    }
    return knocked_in;
}

} // namespace kiyosi::detail
