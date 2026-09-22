#pragma once

#include <utility>

#include <kiyosi/instruments/exercise_based_option.hpp>

namespace kiyosi {

/// European option paying fixed cash when it expires in the money.
using CashOrNothingOption = ExerciseBasedOption<CashOrNothingPayoff, EuropeanExercise>;
/// European option paying the underlying asset when it expires in the money.
using AssetOrNothingOption = ExerciseBasedOption<AssetOrNothingPayoff, EuropeanExercise>;

/// Creates a validated cash-or-nothing European option.
/// @param option_type Call-or-put direction.
/// @param strike Positive strike price.
/// @param payout Positive finite cash amount paid in the money.
/// @param effective_date First date of the option life.
/// @param expiry_date Final date of the option life.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<CashOrNothingOption> make_cash_or_nothing_option(
    OptionType option_type, double strike, double payout, Date effective_date, Date expiry_date)
{
    auto payoff = detail::make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    return detail::make_option(option_type, strike, effective_date, expiry_date, std::move(*payoff), EuropeanExercise{});
}

/// Creates a validated asset-or-nothing European option.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<AssetOrNothingOption> make_asset_or_nothing_option(
    OptionType option_type, double strike, Date effective_date, Date expiry_date)
{
    return detail::make_option(option_type, strike, effective_date, expiry_date, AssetOrNothingPayoff{},
                               EuropeanExercise{});
}

} // namespace kiyosi
