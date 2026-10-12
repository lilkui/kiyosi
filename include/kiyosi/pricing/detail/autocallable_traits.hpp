#pragma once

#include <algorithm>
#include <cmath>
#include <concepts>
#include <chrono>
#include <cstddef>
#include <optional>
#include <vector>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/autocallable/phoenix.hpp>
#include <kiyosi/instruments/autocallable/snowball.hpp>

#include <kiyosi/pricing/settings/implied.hpp>

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
    const auto observations = note.observations_view();
    const auto& observation = observations[index];
    const Date period_start = index == 0 ? note.effective_date() : observations[index - 1].date;
    return {observation.knock_out_level,
            note.coupon_rate() * actual_365_fixed_year_fraction(period_start, observation.date),
            observation.coupon_barrier_level, true, true};
}

template <typename Note>
AutocallableEvent autocallable_event(const Note& note, std::size_t index)
{
    const auto& observation = note.observations_view()[index];
    return {observation.knock_out_level,
            observation.coupon_rate *
                actual_365_fixed_year_fraction(note.effective_date(), observation.date),
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

// Current observations can make spot derivatives undefined while rate and volatility derivatives remain meaningful.
template <AutocallableProduct Note>
bool at_spot_discontinuity(const Note& option, const PricingContext& context, double radius = 0.0)
{
    if (context.valuation_time() != start_of_day(context.valuation_date()) ||
        option.barrier_state() == AutocallableBarrierState::knocked_out) return false;
    const double spot = context.spot_price();
    const auto crosses = [&](double level) { return spot - radius <= level && level <= spot + radius; };
    const auto& dates = option.observation_dates();
    const auto found = std::lower_bound(dates.begin(), dates.end(), context.valuation_date());
    if (found != dates.end() && *found == context.valuation_date()) {
        const auto index = static_cast<std::size_t>(found - dates.begin());
        if (crosses(option.knock_out_levels()[index])) return true;
        if (spot > option.knock_out_levels()[index]) return false;
        if constexpr (std::same_as<Note, PhoenixOption>)
            if (option.coupon_rate() != 0.0 && crosses(option.coupon_barrier_levels()[index])) return true;
    }
    if constexpr (!std::same_as<Note, BinarySnowballOption>)
        // Pricing has already validated history; absent initial history means not knocked in.
        return option.barrier_state() != AutocallableBarrierState::knocked_in &&
               program_knock_in_affects_payoff(autocallable_program(option)) &&
               option.knock_in_observation_mode() == KnockInObservationMode::every_trading_day &&
               context.calendar().is_trading_day(context.valuation_date()) && crosses(option.knock_in_level());
    return false;
}

template <AutocallableProduct Note>
Result<bool> time_stencil_available(const Note& option, const PricingContext& context, Timestamp before, Timestamp after)
{
    const auto crosses_event = [&](Date date) {
        const Timestamp event = start_of_day(date);
        return before <= event && event <= after;
    };
    bool available = true;
    if (option.barrier_state() == AutocallableBarrierState::knocked_out) {
        const auto shifted = make_pricing_context(context.model_parameters(), context.spot_price(), before, context.calendar());
        if (!shifted) return std::unexpected(shifted.error());
        available = validate_autocallable_history(option, *shifted).has_value();
    } else {
        available = std::ranges::none_of(option.observation_dates(), crosses_event);
    }
    if constexpr (!std::same_as<Note, BinarySnowballOption>) {
        const bool daily_events = option.knock_in_observation_mode() == KnockInObservationMode::every_trading_day &&
                                  option.barrier_state() != AutocallableBarrierState::knocked_in &&
                                  option.barrier_state() != AutocallableBarrierState::knocked_out &&
                                  program_knock_in_affects_payoff(autocallable_program(option));
        if (daily_events)
            for (Date date = date_of(before); date <= date_of(after); date += std::chrono::days{1})
                if (crosses_event(date) && context.calendar().is_trading_day(date)) return false;
    }
    return available;
}

template <AutocallableProduct Note>
bool volatility_affects_price(const Note& option, const PricingContext& context)
{
    const auto program = autocallable_program(option);
    const auto initial = autocallable_initial_state(option, context, program);
    return !initial.settlement && !autocallable_fixed_value(option, context, program, initial);
}

inline Result<double> shifted_maturity_coupon(
    double maturity_coupon_rate, double shift, CouponQuoteConvention convention)
{
    switch (convention) {
    case CouponQuoteConvention::shift_maturity_coupon:
        return maturity_coupon_rate + shift;
    case CouponQuoteConvention::preserve_maturity_coupon:
        return maturity_coupon_rate;
    }
    return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                 "coupon quote convention is invalid"});
}

template <std::ranges::input_range Rates>
std::vector<double> shifted_coupon_rates(
    const Rates& coupon_rates, double coupon)
{
    auto result = coupon_rates | std::ranges::to<std::vector<double>>();
    const double shift = coupon - result.front();
    for (double& rate : result)
        rate += shift;
    return result;
}

inline Result<SnowballOption> replace_coupon(
    const SnowballOption& option, double coupon, CouponQuoteConvention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon_rate = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon_rate) return std::unexpected(maturity_coupon_rate.error());
    return make_snowball_option({.knock_out_coupon_rates = shifted_coupon_rates(
                                     option.knock_out_coupon_rates(), coupon),
                                 .maturity_coupon_rate = *maturity_coupon_rate,
                                 .initial_spot = option.initial_spot(),
                                 .knock_in_level = option.knock_in_level(),
                                 .knock_out_levels = option.knock_out_levels() | std::ranges::to<std::vector<double>>(),
                                 .upper_strike = option.upper_strike(),
                                 .lower_strike = option.lower_strike(),
                                 .observation_dates = option.observation_dates() | std::ranges::to<std::vector<Date>>(),
                                 .knock_in_observation_mode = option.knock_in_observation_mode(),
                                 .barrier_state = option.barrier_state(),
                                 .principal_ratio = option.principal_ratio(),
                                 .effective_date = option.effective_date(),
                                 .expiry_date = option.expiry_date()});
}

inline Result<BinarySnowballOption> replace_coupon(
    const BinarySnowballOption& option, double coupon, CouponQuoteConvention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon_rate = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon_rate) return std::unexpected(maturity_coupon_rate.error());
    return make_binary_snowball_option({.knock_out_coupon_rates = shifted_coupon_rates(option.knock_out_coupon_rates(), coupon),
                                        .maturity_coupon_rate = *maturity_coupon_rate,
                                        .knock_out_levels = option.knock_out_levels() | std::ranges::to<std::vector<double>>(),
                                        .observation_dates = option.observation_dates() | std::ranges::to<std::vector<Date>>(),
                                        .barrier_state = option.barrier_state(),
                                        .principal_ratio = option.principal_ratio(),
                                        .effective_date = option.effective_date(),
                                        .expiry_date = option.expiry_date()});
}

inline Result<TernarySnowballOption> replace_coupon(
    const TernarySnowballOption& option, double coupon, CouponQuoteConvention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon_rate = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon_rate) return std::unexpected(maturity_coupon_rate.error());
    return make_ternary_snowball_option({.knock_out_coupon_rates = shifted_coupon_rates(option.knock_out_coupon_rates(), coupon),
                                         .maturity_coupon_rate = *maturity_coupon_rate,
                                         .minimum_coupon_rate = option.minimum_coupon_rate(),
                                         .knock_in_level = option.knock_in_level(),
                                         .knock_out_levels = option.knock_out_levels() | std::ranges::to<std::vector<double>>(),
                                         .observation_dates = option.observation_dates() | std::ranges::to<std::vector<Date>>(),
                                         .knock_in_observation_mode = option.knock_in_observation_mode(),
                                         .barrier_state = option.barrier_state(),
                                         .principal_ratio = option.principal_ratio(),
                                         .effective_date = option.effective_date(),
                                         .expiry_date = option.expiry_date()});
}

inline Result<PhoenixOption> replace_coupon(const PhoenixOption& option, double coupon)
{
    return make_phoenix_option({.coupon_rate = coupon,
                                .initial_spot = option.initial_spot(),
                                .knock_in_level = option.knock_in_level(),
                                .knock_out_levels = option.knock_out_levels() | std::ranges::to<std::vector<double>>(),
                                .coupon_barrier_levels = option.coupon_barrier_levels() | std::ranges::to<std::vector<double>>(),
                                .upper_strike = option.upper_strike(),
                                .lower_strike = option.lower_strike(),
                                .observation_dates = option.observation_dates() | std::ranges::to<std::vector<Date>>(),
                                .knock_in_observation_mode = option.knock_in_observation_mode(),
                                .barrier_state = option.barrier_state(),
                                .principal_ratio = option.principal_ratio(),
                                .effective_date = option.effective_date(),
                                .expiry_date = option.expiry_date()});
}

} // namespace kiyosi::detail
