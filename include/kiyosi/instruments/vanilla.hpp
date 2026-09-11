#pragma once

#include <kiyosi/instruments/exercise_based_option.hpp>

namespace kiyosi {

using EuropeanOption = ExerciseBasedOption<VanillaPayoff, EuropeanExercise>;
using AmericanOption = ExerciseBasedOption<VanillaPayoff, AmericanExercise>;
using BermudanOption = ExerciseBasedOption<VanillaPayoff, BermudanExercise>;

[[nodiscard]] inline result<EuropeanOption> make_european_option(option_type type, double strike, date expiry)
{ return make_option(type, strike, default_effective_date, expiry, VanillaPayoff{}, EuropeanExercise{}); }
[[nodiscard]] inline result<AmericanOption> make_american_option(option_type type, double strike, date expiry)
{ return make_option(type, strike, default_effective_date, expiry, VanillaPayoff{}, AmericanExercise{}); }
[[nodiscard]] inline result<BermudanOption> make_bermudan_option(option_type type, double strike, date expiry, std::vector<date> dates, const TradingCalendar& calendar = all_days_calendar())
{ auto terms = make_option_terms(type, strike, expiry); if (!terms) return std::unexpected(terms.error()); return make_bermudan_option(*terms, VanillaPayoff{}, std::move(dates), calendar); }

[[nodiscard]] inline result<EuropeanOption> make_european_option(
    option_type type, double strike, date effective, date expiry)
{ return make_option(type, strike, effective, expiry, VanillaPayoff{}, EuropeanExercise{}); }

[[nodiscard]] inline result<AmericanOption> make_american_option(
    option_type type, double strike, date effective, date expiry)
{ return make_option(type, strike, effective, expiry, VanillaPayoff{}, AmericanExercise{}); }

[[nodiscard]] inline result<BermudanOption> make_bermudan_option(
    option_type type, double strike, date valuation_date, date expiry, std::vector<date> dates,
    const TradingCalendar& calendar = all_days_calendar())
{
    auto terms = make_option_terms(type, strike, valuation_date, expiry);
    if (!terms) return std::unexpected(terms.error());
    return make_bermudan_option(*terms, VanillaPayoff{}, std::move(dates), calendar);
}

[[nodiscard]] inline result<EuropeanOption> make_european_call(double strike, date expiry)
{ return make_european_option(option_type::call, strike, default_effective_date, expiry); }
[[nodiscard]] inline result<EuropeanOption> make_european_call(double strike, date effective, date expiry)
{ return make_european_option(option_type::call, strike, effective, expiry); }
[[nodiscard]] inline result<EuropeanOption> make_european_put(double strike, date expiry)
{ return make_european_option(option_type::put, strike, default_effective_date, expiry); }
[[nodiscard]] inline result<EuropeanOption> make_european_put(double strike, date effective, date expiry)
{ return make_european_option(option_type::put, strike, effective, expiry); }
[[nodiscard]] inline result<AmericanOption> make_american_call(double strike, date expiry)
{ return make_american_option(option_type::call, strike, default_effective_date, expiry); }
[[nodiscard]] inline result<AmericanOption> make_american_call(double strike, date effective, date expiry)
{ return make_american_option(option_type::call, strike, effective, expiry); }
[[nodiscard]] inline result<AmericanOption> make_american_put(double strike, date expiry)
{ return make_american_option(option_type::put, strike, default_effective_date, expiry); }
[[nodiscard]] inline result<AmericanOption> make_american_put(double strike, date effective, date expiry)
{ return make_american_option(option_type::put, strike, effective, expiry); }

} // namespace kiyosi
