#pragma once

#include <algorithm>
#include <cmath>
#include <concepts>
#include <cstddef>
#include <optional>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/autocallable/phoenix.hpp>
#include <kiyosi/instruments/autocallable/snowball.hpp>

#include "autocallable_program.hpp"

namespace kiyosi::detail {

/// Per-product settlement and coupon rules shared by the autocallable engines.
inline AutocallableProgram autocallable_program(const PhoenixOption& note)
{
    return {note.principal_ratio(), note.initial_spot(), note.upper_strike(),
            note.lower_strike(), note.knock_in_level(), 0.0, 0.0,
            AutocallableTerminalKind::downside_if_knocked_in, true,
            note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day, true};
}

inline AutocallableProgram autocallable_program(const SnowballOption& note)
{
    return {note.principal_ratio(), note.initial_spot(), note.upper_strike(),
            note.lower_strike(), note.knock_in_level(),
            note.maturity_coupon_rate() * actual_365_fixed_year_fraction(note.effective_date(), note.expiry_date()), 0.0,
            AutocallableTerminalKind::downside_if_knocked_in, true,
            note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day, false};
}

inline AutocallableProgram autocallable_program(const TernarySnowballOption& note)
{
    const double term = actual_365_fixed_year_fraction(note.effective_date(), note.expiry_date());
    return {note.principal_ratio(), 0.0, 0.0,
            0.0, note.knock_in_level(), note.maturity_coupon_rate() * term,
            note.minimum_coupon_rate() * term, AutocallableTerminalKind::fixed, true,
            note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day, false};
}

inline AutocallableProgram autocallable_program(const BinarySnowballOption& note)
{
    const double coupon =
        note.maturity_coupon_rate() * actual_365_fixed_year_fraction(note.effective_date(), note.expiry_date());
    return {note.principal_ratio(), 0.0, 0.0,
            0.0, 0.0, coupon, coupon,
            AutocallableTerminalKind::fixed, false, false, false};
}

inline AutocallableEvent autocallable_event(const PhoenixOption& note, std::size_t index)
{
    const Date period_start = index == 0 ? note.effective_date() : note.observation_dates()[index - 1];
    return {note.knock_out_levels()[index],
            note.coupon_rate() * actual_365_fixed_year_fraction(period_start, note.observation_dates()[index]),
            note.coupon_barrier_levels()[index], true, true};
}

template <typename Note>
AutocallableEvent autocallable_event(const Note& note, std::size_t index)
{
    return {note.knock_out_levels()[index],
            note.knock_out_coupon_rates()[index] *
                actual_365_fixed_year_fraction(note.effective_date(), note.observation_dates()[index]),
            0.0, false, true};
}

// Current events are resolved at the observed spot before numerical continuation.
struct AutocallableInitialState {
    AutocallablePathState path{};
    std::size_t next_observation{};
    std::optional<double> settlement{};
};

template <typename Note>
AutocallableInitialState autocallable_initial_state(const Note& note, const PricingContext& context,
                                                    const AutocallableProgram& program)
{
    if (note.barrier_state() == AutocallableBarrierState::knocked_out)
        return {.settlement = 0.0};

    const Timestamp valuation_time = context.valuation_time();
    const double value = context.spot_price();
    const auto& dates = note.observation_dates();
    AutocallableInitialState initial{
        .path = {.coupons = 0.0,
                 .knocked_in = note.barrier_state() == AutocallableBarrierState::knocked_in},
        .next_observation = static_cast<std::size_t>(
            std::lower_bound(dates.begin(), dates.end(), valuation_time) - dates.begin())};
    if (program.has_knock_in && program.daily_knock_in &&
        valuation_time == start_of_day(date_of(valuation_time)) &&
        context.calendar().is_trading_day(date_of(valuation_time)))
        initial.path.knocked_in = program_knocked_in(
            program, value, initial.path.knocked_in, valuation_time == note.expiry_date());

    if (initial.next_observation < dates.size() && dates[initial.next_observation] == valuation_time) {
        const auto event = autocallable_event(note, initial.next_observation);
        const double coupon = program_observation_coupon(event, value);
        if (value >= event.knock_out_level)
            return {.path = initial.path,
                    .next_observation = initial.next_observation + 1,
                    .settlement = program.principal_ratio + coupon};
        if (program.carries_observation_coupon) initial.path.coupons = coupon;
        ++initial.next_observation;
    }
    if (valuation_time == note.expiry_date()) {
        initial.path.knocked_in =
            program_knocked_in(program, value, initial.path.knocked_in, true);
        initial.settlement = initial.path.coupons +
                             program_terminal_settlement(
                                 program, value, initial.path.knocked_in);
    }
    return initial;
}

// Callers validate the note and history and resolve initial settlements first.
template <typename Note>
std::optional<double> autocallable_fixed_value(const Note& note, const PricingContext& context,
                                               const AutocallableProgram& program,
                                               const AutocallableInitialState& initial)
{
    const bool knocked_in = initial.path.knocked_in;
    if constexpr (requires { note.knock_in_level(); }) {
        if constexpr (std::same_as<Note, TernarySnowballOption>) {
            if (!knocked_in && note.minimum_coupon_rate() != note.maturity_coupon_rate()) return std::nullopt;
        } else {
            if (program.lower_strike != program.upper_strike) return std::nullopt;
            if constexpr (std::same_as<Note, SnowballOption>)
                if (!knocked_in && note.maturity_coupon_rate() != 0.0) return std::nullopt;
        }
    }
    const double rate = context.model_parameters().risk_free_rate();
    const double terminal_coupon = knocked_in ? program.knocked_in_terminal_coupon : program.intact_terminal_coupon;
    const double terminal_discount = std::exp(-rate * actual_365_fixed_year_fraction(context.valuation_time(), note.expiry_date()));
    // Underflowed weights cannot establish equality between distinct cashflows.
    if (!std::isnormal(terminal_discount)) return std::nullopt;
    const double maturity_value = (program.principal_ratio + terminal_coupon) * terminal_discount;
    if (!std::isfinite(maturity_value)) return std::nullopt;
    double remaining_coupon_value = 0.0;
    for (std::size_t i = note.observation_dates().size(); i-- > initial.next_observation;) {
        const auto event = autocallable_event(note, i);
        const double discount = std::exp(-rate * actual_365_fixed_year_fraction(context.valuation_time(), note.observation_dates()[i]));
        if (!std::isnormal(discount)) return std::nullopt;
        // Phoenix pays the current coupon in either branch; knock-out loses only later coupons.
        const double knock_out_value = (program.principal_ratio + (program.carries_observation_coupon ? 0.0 : event.coupon)) * discount;
        if (!std::isfinite(knock_out_value) || knock_out_value != maturity_value + remaining_coupon_value) return std::nullopt;
        if (program.carries_observation_coupon) {
            if (event.coupon != 0.0 && event.coupon_barrier != 0.0) return std::nullopt;
            remaining_coupon_value += event.coupon * discount;
            if (!std::isfinite(remaining_coupon_value)) return std::nullopt;
        }
    }
    const double value = initial.path.coupons + maturity_value + remaining_coupon_value;
    return std::isfinite(value) ? std::optional{value} : std::nullopt;
}

} // namespace kiyosi::detail
