#pragma once

#include <utility>

#include <kiyosi/instruments/exercise_based_option.hpp>

namespace kiyosi {

using EuropeanCashOrNothingOption = ExerciseBasedOption<CashOrNothingPayoff, EuropeanExercise>;
using EuropeanAssetOrNothingOption = ExerciseBasedOption<AssetOrNothingPayoff, EuropeanExercise>;

[[nodiscard]] inline result<EuropeanCashOrNothingOption> make_cash_or_nothing_option(
    option_type type, double strike, double payout, date effective, date expiry)
{
    auto payoff = detail::make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    return detail::make_option(type, strike, effective, expiry, std::move(*payoff), EuropeanExercise{});
}

[[nodiscard]] inline result<EuropeanAssetOrNothingOption> make_asset_or_nothing_option(
    option_type type, double strike, date effective, date expiry)
{
    return detail::make_option(type, strike, effective, expiry, AssetOrNothingPayoff{},
                               EuropeanExercise{});
}

} // namespace kiyosi
