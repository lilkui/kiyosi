#pragma once

#include <kiyosi/instruments/structured.hpp>
#include <kiyosi/market/context.hpp>
#include "common.hpp"
#include <algorithm>
#include <type_traits>
#include <vector>

namespace kiyosi::detail {

template <typename Option>
struct structured_product_traits;

template <>
struct structured_product_traits<PhoenixOption> {
    static constexpr bool carries_observation_coupon = true;
    static double terminal_settlement(const PhoenixOption& option, double spot, bool knocked_in)
    {
        const double loss = std::clamp(spot - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) /
                            option.initial_price();
        return option.principal_ratio() + (knocked_in ? loss : 0.0);
    }
    static double observation_coupon(const PhoenixOption& option, std::size_t index, double spot)
    { return spot >= option.coupon_barriers()[index] ? option.initial_price() * option.coupon_rate() : 0.0; }
};

template <>
struct structured_product_traits<SnowballOption> {
    static constexpr bool carries_observation_coupon = false;
    static double terminal_settlement(const SnowballOption& option, double spot, bool knocked_in)
    {
        const double loss = std::clamp(spot - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) /
                            option.initial_price();
        const double coupon = knocked_in ? loss : option.maturity_coupon_rate() * actual_365(option.effective(), option.expiry());
        return option.principal_ratio() + coupon;
    }
    static double observation_coupon(const SnowballOption& option, std::size_t index, double)
    { return option.knock_out_coupon_rates()[index] * actual_365(option.effective(), option.observation_dates()[index]); }
};

template <>
struct structured_product_traits<TernarySnowballOption> {
    static constexpr bool carries_observation_coupon = false;
    static double terminal_settlement(const TernarySnowballOption& option, double, bool knocked_in)
    {
        const double rate = knocked_in ? option.minimal_coupon_rate() : option.maturity_coupon_rate();
        return option.principal_ratio() + rate * actual_365(option.effective(), option.expiry());
    }
    static double observation_coupon(const TernarySnowballOption& option, std::size_t index, double)
    { return option.knock_out_coupon_rates()[index] * actual_365(option.effective(), option.observation_dates()[index]); }
};

template <>
struct structured_product_traits<BinarySnowballOption> {
    static constexpr bool carries_observation_coupon = false;
    static double terminal_settlement(const BinarySnowballOption& option, double, bool)
    { return option.principal_ratio() + option.maturity_coupon_rate() * actual_365(option.effective(), option.expiry()); }
    static double observation_coupon(const BinarySnowballOption& option, std::size_t index, double)
    { return option.knock_out_coupon_rates()[index] * actual_365(option.effective(), option.observation_dates()[index]); }
};

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
    return structured_product_traits<Option>::terminal_settlement(option, spot, knocked_in);
}

template <typename Option>
double observation_coupon(const Option& option, std::size_t index, double spot)
{
    return structured_product_traits<Option>::observation_coupon(option, index, spot);
}

template <typename Option>
inline constexpr bool carries_observation_coupon = structured_product_traits<Option>::carries_observation_coupon;

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
