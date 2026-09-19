#pragma once

#include <span>
#include <utility>

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
[[nodiscard]] Result<ExerciseBasedOption<Payoff, Exercise>> make_exercise_based_option(
    OptionTerms, Payoff, Exercise);

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] Result<ExerciseBasedOption<Payoff, Exercise>> make_option(
    OptionType, double, Date, Date, Payoff, Exercise);

} // namespace detail

/// An option built from an independent payoff and exercise style over shared option terms.
template <OptionPayoff Payoff, OptionExercise Exercise>
class ExerciseBasedOption {
public:
    OptionType option_type() const noexcept { return terms_.option_type(); }
    double strike() const noexcept { return terms_.strike(); }
    Date effective_date() const noexcept { return terms_.effective_date(); }
    Date expiry_date() const noexcept { return terms_.expiry_date(); }
    const OptionTerms& terms() const noexcept { return terms_; }
    const Payoff& payoff() const noexcept { return payoff_; }
    const Exercise& exercise() const noexcept { return exercise_; }

    double payout() const noexcept
        requires requires(const Payoff& value) { value.payout(); }
    {
        return payoff_.payout();
    }

    friend bool operator==(const ExerciseBasedOption&, const ExerciseBasedOption&) = default;

private:
    ExerciseBasedOption(OptionTerms terms, Payoff payoff, Exercise exercise)
        : terms_(std::move(terms)), payoff_(std::move(payoff)), exercise_(std::move(exercise)) {}

    OptionTerms terms_;
    Payoff payoff_;
    Exercise exercise_;

    template <OptionPayoff OtherPayoff, OptionExercise OtherExercise>
    friend Result<ExerciseBasedOption<OtherPayoff, OtherExercise>> detail::make_exercise_based_option(
        OptionTerms, OtherPayoff, OtherExercise);
};

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline Result<ExerciseBasedOption<Payoff, Exercise>> detail::make_exercise_based_option(
    OptionTerms terms, Payoff payoff, Exercise exercise)
{
    return ExerciseBasedOption<Payoff, Exercise>{std::move(terms), std::move(payoff), std::move(exercise)};
}

namespace detail {

template <OptionPayoff Payoff>
[[nodiscard]] inline Result<ExerciseBasedOption<Payoff, EuropeanExercise>> make_european_option(
    OptionTerms terms, Payoff payoff)
{
    return detail::make_exercise_based_option(std::move(terms), std::move(payoff), EuropeanExercise{});
}

template <OptionPayoff Payoff>
[[nodiscard]] inline Result<ExerciseBasedOption<Payoff, AmericanExercise>> make_american_option(
    OptionTerms terms, Payoff payoff)
{
    return detail::make_exercise_based_option(std::move(terms), std::move(payoff), AmericanExercise{});
}

} // namespace detail

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline Result<ExerciseBasedOption<Payoff, Exercise>> detail::make_option(
    OptionType option_type, double strike, Date effective_date, Date expiry_date, Payoff payoff, Exercise exercise)
{
    auto terms = detail::make_option_terms(option_type, strike, effective_date, expiry_date);
    if (!terms) return std::unexpected(terms.error());
    return detail::make_exercise_based_option(*terms, std::move(payoff), std::move(exercise));
}

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline Result<void> validate_observation_dates(
    std::span<const Date> observation_dates, Date valuation_date,
    const ExerciseBasedOption<Payoff, Exercise>& option, const TradingCalendar& calendar)
{
    return validate_observation_dates(observation_dates, valuation_date, option.expiry_date(), calendar);
}

} // namespace kiyosi
