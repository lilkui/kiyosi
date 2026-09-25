#pragma once

#include <cstddef>
#include <vector>

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>

#include "math.hpp"
#include "autocallable_program.hpp"

namespace kiyosi::detail {

/// Per-product settlement and coupon rules; every autocallable engine drives the shared
/// backward induction through this trait so the products stay free of pricing logic.
template <typename Note>
struct AutocallableTraits;

template <>
struct AutocallableTraits<PhoenixOption> {
    static constexpr bool carries_observation_coupon = true;

    static AutocallableProgram program(const PhoenixOption& note)
    {
        return {note.principal_ratio(), note.initial_spot(), note.upper_strike(),
                note.lower_strike(), note.knock_in_level(), 0.0, 0.0,
                AutocallableTerminalKind::downside_if_knocked_in, true,
                note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day, true};
    }

    static AutocallableEvent event(const PhoenixOption& note, std::size_t index)
    {
        const Date period_start = index == 0 ? note.effective_date() : note.observation_dates()[index - 1];
        return {note.knock_out_levels()[index],
                note.coupon_rate() * actual_365_fixed_year_fraction(period_start, note.observation_dates()[index]),
                note.coupon_barrier_levels()[index], true, true};
    }
};

template <>
struct AutocallableTraits<SnowballOption> {
    static constexpr bool carries_observation_coupon = false;

    static AutocallableProgram program(const SnowballOption& note)
    {
        return {note.principal_ratio(), note.initial_spot(), note.upper_strike(),
                note.lower_strike(), note.knock_in_level(),
                note.maturity_coupon_rate() * actual_365_fixed_year_fraction(note.effective_date(), note.expiry_date()), 0.0,
                AutocallableTerminalKind::downside_if_knocked_in, true,
                note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day, false};
    }

    static AutocallableEvent event(const SnowballOption& note, std::size_t index)
    {
        return {note.knock_out_levels()[index],
                note.knock_out_coupon_rates()[index] *
                    actual_365_fixed_year_fraction(note.effective_date(), note.observation_dates()[index]),
                0.0, false, true};
    }
};

template <>
struct AutocallableTraits<TernarySnowballOption> {
    static constexpr bool carries_observation_coupon = false;

    static AutocallableProgram program(const TernarySnowballOption& note)
    {
        const double term = actual_365_fixed_year_fraction(note.effective_date(), note.expiry_date());
        return {note.principal_ratio(), note.initial_spot(), note.upper_strike(),
                note.lower_strike(), note.knock_in_level(), note.maturity_coupon_rate() * term,
                note.minimum_coupon_rate() * term, AutocallableTerminalKind::fixed, true,
                note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day, false};
    }

    static AutocallableEvent event(const TernarySnowballOption& note, std::size_t index)
    {
        return {note.knock_out_levels()[index],
                note.knock_out_coupon_rates()[index] *
                    actual_365_fixed_year_fraction(note.effective_date(), note.observation_dates()[index]),
                0.0, false, true};
    }
};

template <>
struct AutocallableTraits<BinarySnowballOption> {
    static constexpr bool carries_observation_coupon = false;

    static AutocallableProgram program(const BinarySnowballOption& note)
    {
        const double coupon =
            note.maturity_coupon_rate() * actual_365_fixed_year_fraction(note.effective_date(), note.expiry_date());
        return {note.principal_ratio(), note.initial_spot(), note.upper_strike(),
                note.lower_strike(), 0.0, coupon, coupon,
                AutocallableTerminalKind::fixed, false, false, false};
    }

    static AutocallableEvent event(const BinarySnowballOption& note, std::size_t index)
    {
        return {note.knock_out_levels()[index],
                note.knock_out_coupon_rates()[index] *
                    actual_365_fixed_year_fraction(note.effective_date(), note.observation_dates()[index]),
                0.0, false, true};
    }
};

template <typename Note>
AutocallableProgram autocallable_program(const Note& note)
{
    return AutocallableTraits<Note>::program(note);
}

template <typename Note>
AutocallableEvent autocallable_event(const Note& note, std::size_t index)
{
    return AutocallableTraits<Note>::event(note, index);
}

template <typename Note>
double terminal_settlement(const Note& note, double spot, bool knocked_in)
{
    return program_terminal_settlement(autocallable_program(note), spot, knocked_in);
}

template <typename Note>
double observation_coupon(const Note& note, std::size_t index, double spot)
{
    return program_observation_coupon(autocallable_event(note, index), spot);
}

template <typename Note>
inline constexpr bool carries_observation_coupon = AutocallableTraits<Note>::carries_observation_coupon;

/// Indices of the observation dates still ahead of `valuation`.
template <typename Note>
std::vector<std::size_t> remaining_observation_indices(const Note& note, Timestamp valuation)
{
    std::vector<std::size_t> schedule;
    const auto& dates = note.observation_dates();
    for (std::size_t index = 0; index < dates.size(); ++index)
        if (dates[index] >= valuation) schedule.push_back(index);
    return schedule;
}

template <typename Note>
bool is_knocked_in(const Note& note, double spot, bool knocked_in, bool at_expiry)
{
    return program_knocked_in(autocallable_program(note), spot, knocked_in, at_expiry);
}

} // namespace kiyosi::detail
