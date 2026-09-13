#pragma once

#include <span>
#include <utility>
#include <vector>

#include <kiyosi/instruments/exercise.hpp>
#include <kiyosi/instruments/option_terms.hpp>
#include <kiyosi/instruments/payoff.hpp>
#include <kiyosi/market/calendar.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

template <OptionPayoff Payoff, OptionExercise Exercise>
class ExerciseBasedOption;

namespace detail {

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] result<ExerciseBasedOption<Payoff, Exercise>> make_exercise_based_option(
    OptionTerms, Payoff, Exercise);

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] result<ExerciseBasedOption<Payoff, Exercise>> make_option(
    option_type, double, date, date, Payoff, Exercise);

} // namespace detail

/// An option built from an independent payoff and exercise style over shared option terms.
template <OptionPayoff Payoff, OptionExercise Exercise>
class ExerciseBasedOption {
public:
    option_type type() const noexcept { return terms_.type(); }
    double strike() const noexcept { return terms_.strike(); }
    date effective() const noexcept { return terms_.effective(); }
    date expiry() const noexcept { return terms_.expiry(); }
    const OptionTerms& terms() const noexcept { return terms_; }
    const Payoff& payoff() const noexcept { return payoff_; }
    const Exercise& exercise() const noexcept { return exercise_; }

    double payout() const noexcept
        requires requires(const Payoff& value) { value.payout(); }
    {
        return payoff_.payout();
    }

    const std::vector<date>& exercise_dates() const noexcept
        requires requires(const Exercise& value) { value.dates(); }
    {
        return exercise_.dates();
    }

    friend bool operator==(const ExerciseBasedOption&, const ExerciseBasedOption&) = default;

private:
    ExerciseBasedOption(OptionTerms terms, Payoff payoff, Exercise exercise)
        : terms_(std::move(terms)), payoff_(std::move(payoff)), exercise_(std::move(exercise)) {}

    OptionTerms terms_;
    Payoff payoff_;
    Exercise exercise_;

    template <OptionPayoff OtherPayoff, OptionExercise OtherExercise>
    friend result<ExerciseBasedOption<OtherPayoff, OtherExercise>> detail::make_exercise_based_option(
        OptionTerms, OtherPayoff, OtherExercise);
};

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, Exercise>> detail::make_exercise_based_option(
    OptionTerms terms, Payoff payoff, Exercise exercise)
{
    if constexpr (std::same_as<Exercise, BermudanExercise>) {
        auto valid = validate_date_schedule(exercise.dates(), terms.effective(), terms.expiry());
        if (!valid)
            return std::unexpected(Error{error_category::invalid_schedule, valid.error().message});
    }
    return ExerciseBasedOption<Payoff, Exercise>{std::move(terms), std::move(payoff), std::move(exercise)};
}

namespace detail {

template <OptionPayoff Payoff>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, EuropeanExercise>> make_european_option(
    OptionTerms terms, Payoff payoff)
{
    return detail::make_exercise_based_option(std::move(terms), std::move(payoff), EuropeanExercise{});
}

template <OptionPayoff Payoff>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, AmericanExercise>> make_american_option(
    OptionTerms terms, Payoff payoff)
{
    return detail::make_exercise_based_option(std::move(terms), std::move(payoff), AmericanExercise{});
}

template <OptionPayoff Payoff>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, BermudanExercise>> make_bermudan_option(
    OptionTerms terms, Payoff payoff, std::vector<date> dates)
{
    auto exercise = detail::make_bermudan_exercise(std::move(dates), terms.expiry());
    if (!exercise) return std::unexpected(exercise.error());
    return detail::make_exercise_based_option(std::move(terms), std::move(payoff), std::move(*exercise));
}

} // namespace detail

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, Exercise>> detail::make_option(
    option_type type, double strike, date effective, date expiry, Payoff payoff, Exercise exercise)
{
    auto terms = detail::make_option_terms(type, strike, effective, expiry);
    if (!terms) return std::unexpected(terms.error());
    return detail::make_exercise_based_option(*terms, std::move(payoff), std::move(exercise));
}

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline result<void> validate_observation_dates(
    std::span<const date> observations, date valuation_date,
    const ExerciseBasedOption<Payoff, Exercise>& option, const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, valuation_date, option.expiry(), calendar);
}

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline result<void> validate_schedule(
    std::span<const date> observations, date valuation_date,
    const ExerciseBasedOption<Payoff, Exercise>& option, const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, valuation_date, option, calendar);
}

} // namespace kiyosi
