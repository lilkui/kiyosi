#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/exercise_based_option.hpp>

namespace kiyosi {

using EuropeanOption = ExerciseBasedOption<VanillaPayoff, EuropeanExercise>;
using AmericanOption = ExerciseBasedOption<VanillaPayoff, AmericanExercise>;
using BermudanOption = ExerciseBasedOption<VanillaPayoff, BermudanExercise>;

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

[[nodiscard]] inline result<BermudanOption> make_bermudan_option(
    option_type type, double strike, date effective, date expiry, std::vector<date> dates)
{
    auto terms = detail::make_option_terms(type, strike, effective, expiry);
    if (!terms) return std::unexpected(terms.error());
    return detail::make_bermudan_option(*terms, VanillaPayoff{}, std::move(dates));
}

} // namespace kiyosi
