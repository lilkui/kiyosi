#pragma once

#include <algorithm>
#include <cstddef>
#include <vector>

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>

#include "math.hpp"

namespace kiyosi::detail {

/// Per-product settlement and coupon rules; every autocallable engine drives the shared
/// backward induction through this trait so the products stay free of pricing logic.
template <typename Note>
struct autocallable_traits;

template <>
struct autocallable_traits<PhoenixOption> {
    static constexpr bool carries_observation_coupon = true;

    static double terminal_settlement(const PhoenixOption& note, double spot, bool knocked_in)
    {
        const double loss =
            std::clamp(spot - note.upper_strike(), note.lower_strike() - note.upper_strike(), 0.0) /
            note.initial_price();
        return note.principal_ratio() + (knocked_in ? loss : 0.0);
    }

    static double observation_coupon(const PhoenixOption& note, std::size_t index, double spot)
    {
        return spot >= note.coupon_barriers()[index] ? note.initial_price() * note.coupon_rate() : 0.0;
    }
};

template <>
struct autocallable_traits<SnowballOption> {
    static constexpr bool carries_observation_coupon = false;

    static double terminal_settlement(const SnowballOption& note, double spot, bool knocked_in)
    {
        const double loss =
            std::clamp(spot - note.upper_strike(), note.lower_strike() - note.upper_strike(), 0.0) /
            note.initial_price();
        const double coupon = knocked_in ? loss
                                         : note.maturity_coupon_rate() *
                                               actual_365(note.effective(), note.expiry());
        return note.principal_ratio() + coupon;
    }

    static double observation_coupon(const SnowballOption& note, std::size_t index, double)
    {
        return note.knock_out_coupon_rates()[index] *
               actual_365(note.effective(), note.observation_dates()[index]);
    }
};

template <>
struct autocallable_traits<TernarySnowballOption> {
    static constexpr bool carries_observation_coupon = false;

    static double terminal_settlement(const TernarySnowballOption& note, double, bool knocked_in)
    {
        const double rate = knocked_in ? note.minimal_coupon_rate() : note.maturity_coupon_rate();
        return note.principal_ratio() + rate * actual_365(note.effective(), note.expiry());
    }

    static double observation_coupon(const TernarySnowballOption& note, std::size_t index, double)
    {
        return note.knock_out_coupon_rates()[index] *
               actual_365(note.effective(), note.observation_dates()[index]);
    }
};

template <>
struct autocallable_traits<BinarySnowballOption> {
    static constexpr bool carries_observation_coupon = false;

    static double terminal_settlement(const BinarySnowballOption& note, double, bool)
    {
        return note.principal_ratio() +
               note.maturity_coupon_rate() * actual_365(note.effective(), note.expiry());
    }

    static double observation_coupon(const BinarySnowballOption& note, std::size_t index, double)
    {
        return note.knock_out_coupon_rates()[index] *
               actual_365(note.effective(), note.observation_dates()[index]);
    }
};

template <typename Note>
double terminal_settlement(const Note& note, double spot, bool knocked_in)
{
    return autocallable_traits<Note>::terminal_settlement(note, spot, knocked_in);
}

template <typename Note>
double observation_coupon(const Note& note, std::size_t index, double spot)
{
    return autocallable_traits<Note>::observation_coupon(note, index, spot);
}

template <typename Note>
inline constexpr bool carries_observation_coupon = autocallable_traits<Note>::carries_observation_coupon;

/// Indices of the observation dates still ahead of `valuation`.
template <typename Note>
std::vector<std::size_t> observation_schedule(const Note& note, timestamp valuation)
{
    std::vector<std::size_t> schedule;
    const auto& dates = note.observation_dates();
    for (std::size_t index = 0; index < dates.size(); ++index)
        if (dates[index] >= valuation) schedule.push_back(index);
    return schedule;
}

template <typename Note>
bool is_knocked_in(const Note& note, double spot, bool knocked_in, bool expiry)
{
    if constexpr (requires { note.knock_in_price(); }) {
        if (note.knock_in_frequency() == observation_frequency::daily || expiry)
            return knocked_in || spot < note.knock_in_price();
    }
    return knocked_in;
}

} // namespace kiyosi::detail
