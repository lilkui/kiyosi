#pragma once

#include <kiyosi/instruments/exercise_based_option.hpp>

namespace kiyosi {

using EuropeanOption = ExerciseBasedOption<VanillaPayoff, EuropeanExercise>;
using AmericanOption = ExerciseBasedOption<VanillaPayoff, AmericanExercise>;

[[nodiscard]] inline Result<EuropeanOption> make_european_option(
    OptionType option_type, double strike, Date effective_date, Date expiry_date)
{
    return detail::make_option(option_type, strike, effective_date, expiry_date, VanillaPayoff{}, EuropeanExercise{});
}

[[nodiscard]] inline Result<AmericanOption> make_american_option(
    OptionType option_type, double strike, Date effective_date, Date expiry_date)
{
    return detail::make_option(option_type, strike, effective_date, expiry_date, VanillaPayoff{}, AmericanExercise{});
}

} // namespace kiyosi
