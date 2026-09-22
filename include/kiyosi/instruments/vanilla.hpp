#pragma once

#include <kiyosi/instruments/exercise_based_option.hpp>

namespace kiyosi {

/// European vanilla call or put.
using EuropeanOption = ExerciseBasedOption<VanillaPayoff, EuropeanExercise>;
/// American vanilla call or put.
using AmericanOption = ExerciseBasedOption<VanillaPayoff, AmericanExercise>;

/// Creates a validated European vanilla option.
/// @return The option, or an `invalid_option`, `invalid_strike`, or `invalid_schedule` error.
[[nodiscard]] inline Result<EuropeanOption> make_european_option(
    OptionType option_type, double strike, Date effective_date, Date expiry_date)
{
    return detail::make_option(option_type, strike, effective_date, expiry_date, VanillaPayoff{}, EuropeanExercise{});
}

/// Creates a validated American vanilla option.
/// @return The option, or an `invalid_option`, `invalid_strike`, or `invalid_schedule` error.
[[nodiscard]] inline Result<AmericanOption> make_american_option(
    OptionType option_type, double strike, Date effective_date, Date expiry_date)
{
    return detail::make_option(option_type, strike, effective_date, expiry_date, VanillaPayoff{}, AmericanExercise{});
}

} // namespace kiyosi
