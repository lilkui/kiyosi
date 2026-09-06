#pragma once

#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

using CashOrNothingOption = EuropeanCashOrNothingOption;
using AssetOrNothingOption = EuropeanAssetOrNothingOption;

template <OptionExercise Exercise>
[[nodiscard]] inline result<ExerciseBasedOption<CashOrNothingPayoff, Exercise>> make_cash_or_nothing_option(
    option_type type, double strike, double payout, date expiry, Exercise exercise)
{
    auto payoff = make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    return make_option(type, strike, expiry, std::move(*payoff), std::move(exercise));
}

[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_option(
    option_type type, double strike, double payout, date expiry)
{
    return make_cash_or_nothing_option(type, strike, payout, expiry, EuropeanExercise{});
}

[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_option(
    option_type type, double strike, double payout, date valuation, date expiry)
{
    auto valid = validate_expiry(valuation, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_cash_or_nothing_option(type, strike, payout, expiry);
}

[[nodiscard]] inline result<AmericanCashOrNothingOption> make_american_cash_or_nothing_option(
    option_type type, double strike, double payout, date expiry)
{
    return make_cash_or_nothing_option(type, strike, payout, expiry, AmericanExercise{});
}

[[nodiscard]] inline result<EuropeanCashOrNothingOption> make_european_cash_or_nothing_option(
    option_type type, double strike, double payout, date expiry)
{
    return make_cash_or_nothing_option(type, strike, payout, expiry);
}

[[nodiscard]] inline result<BermudanCashOrNothingOption> make_bermudan_cash_or_nothing_option(
    option_type type, double strike, double payout, date expiry, std::vector<date> dates,
    const TradingCalendar& calendar = all_days_calendar())
{
    auto exercise = make_bermudan_exercise(std::move(dates), expiry, calendar);
    if (!exercise) return std::unexpected(exercise.error());
    return make_cash_or_nothing_option(type, strike, payout, expiry, std::move(*exercise));
}

[[nodiscard]] inline result<BermudanCashOrNothingOption> make_bermudan_cash_or_nothing_option(
    option_type type, double strike, double payout, date valuation, date expiry, std::vector<date> dates,
    const TradingCalendar& calendar = all_days_calendar())
{
    auto valid = validate_expiry(valuation, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_bermudan_cash_or_nothing_option(type, strike, payout, expiry, std::move(dates), calendar);
}

[[nodiscard]] inline result<AmericanCashOrNothingOption> make_american_cash_or_nothing_option(
    option_type type, double strike, double payout, date valuation, date expiry)
{
    auto valid = validate_expiry(valuation, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_american_cash_or_nothing_option(type, strike, payout, expiry);
}

[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_call(double strike, double payout, date expiry)
{
    return make_cash_or_nothing_option(option_type::call, strike, payout, expiry);
}
[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_put(double strike, double payout, date expiry)
{
    return make_cash_or_nothing_option(option_type::put, strike, payout, expiry);
}

template <OptionExercise Exercise>
[[nodiscard]] inline result<ExerciseBasedOption<AssetOrNothingPayoff, Exercise>> make_asset_or_nothing_option(
    option_type type, double strike, date expiry, Exercise exercise)
{
    return make_option(type, strike, expiry, AssetOrNothingPayoff{}, std::move(exercise));
}

[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_option(
    option_type type, double strike, date expiry)
{
    return make_asset_or_nothing_option(type, strike, expiry, EuropeanExercise{});
}

[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_option(
    option_type type, double strike, date valuation, date expiry)
{
    auto valid = validate_expiry(valuation, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_asset_or_nothing_option(type, strike, expiry);
}

[[nodiscard]] inline result<AmericanAssetOrNothingOption> make_american_asset_or_nothing_option(
    option_type type, double strike, date expiry)
{
    return make_asset_or_nothing_option(type, strike, expiry, AmericanExercise{});
}

[[nodiscard]] inline result<EuropeanAssetOrNothingOption> make_european_asset_or_nothing_option(
    option_type type, double strike, date expiry)
{
    return make_asset_or_nothing_option(type, strike, expiry);
}

[[nodiscard]] inline result<BermudanAssetOrNothingOption> make_bermudan_asset_or_nothing_option(
    option_type type, double strike, date expiry, std::vector<date> dates,
    const TradingCalendar& calendar = all_days_calendar())
{
    auto exercise = make_bermudan_exercise(std::move(dates), expiry, calendar);
    if (!exercise) return std::unexpected(exercise.error());
    return make_asset_or_nothing_option(type, strike, expiry, std::move(*exercise));
}

[[nodiscard]] inline result<BermudanAssetOrNothingOption> make_bermudan_asset_or_nothing_option(
    option_type type, double strike, date valuation, date expiry, std::vector<date> dates,
    const TradingCalendar& calendar = all_days_calendar())
{
    auto valid = validate_expiry(valuation, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_bermudan_asset_or_nothing_option(type, strike, expiry, std::move(dates), calendar);
}

[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_call(double strike, date expiry)
{
    return make_asset_or_nothing_option(option_type::call, strike, expiry);
}
[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_put(double strike, date expiry)
{
    return make_asset_or_nothing_option(option_type::put, strike, expiry);
}

} // namespace kiyosi
