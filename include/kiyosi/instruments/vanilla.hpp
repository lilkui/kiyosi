#pragma once

#include <kiyosi/instruments/exercise_based_option.hpp>

namespace kiyosi {

using EuropeanOption = ExerciseBasedOption<VanillaPayoff, EuropeanExercise>;
using AmericanOption = ExerciseBasedOption<VanillaPayoff, AmericanExercise>;

[[nodiscard]] inline result<EuropeanOption> make_european_option(
    option_type type, double strike, date effective, date expiry)
{
    return detail::make_option(type, strike, effective, expiry, VanillaPayoff{}, EuropeanExercise{});
}

[[nodiscard]] inline result<AmericanOption> make_american_option(
    option_type type, double strike, date effective, date expiry)
{
    return detail::make_option(type, strike, effective, expiry, VanillaPayoff{}, AmericanExercise{});
}

} // namespace kiyosi
