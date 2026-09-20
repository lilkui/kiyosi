#pragma once

#include <utility>

#include <kiyosi/instruments/exercise_based_option.hpp>

namespace kiyosi {

using CashOrNothingOption = ExerciseBasedOption<CashOrNothingPayoff, EuropeanExercise>;
using AssetOrNothingOption = ExerciseBasedOption<AssetOrNothingPayoff, EuropeanExercise>;

[[nodiscard]] inline Result<CashOrNothingOption> make_cash_or_nothing_option(
    OptionType option_type, double strike, double payout, Date effective_date, Date expiry_date)
{
    auto payoff = detail::make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    return detail::make_option(option_type, strike, effective_date, expiry_date, std::move(*payoff), EuropeanExercise{});
}

[[nodiscard]] inline Result<AssetOrNothingOption> make_asset_or_nothing_option(
    OptionType option_type, double strike, Date effective_date, Date expiry_date)
{
    return detail::make_option(option_type, strike, effective_date, expiry_date, AssetOrNothingPayoff{},
                               EuropeanExercise{});
}

} // namespace kiyosi
