#pragma once

#include <utility>

#include <kiyosi/instruments/exercise.hpp>
#include <kiyosi/instruments/option_terms.hpp>
#include <kiyosi/instruments/payoff.hpp>

namespace kiyosi {

template <OptionPayoff Payoff, OptionExercise Exercise>
class ExerciseBasedOption;

namespace detail {

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] Result<ExerciseBasedOption<Payoff, Exercise>> make_option(
    OptionType, double, Date, Date, Payoff, Exercise);

} // namespace detail

/// An option built from an independent payoff and exercise style over shared option terms.
/// @tparam Payoff Copyable payoff tag satisfying OptionPayoff.
/// @tparam Exercise Copyable exercise-style tag satisfying OptionExercise.
template <OptionPayoff Payoff, OptionExercise Exercise>
class ExerciseBasedOption {
public:
    /// Returns the call-or-put direction.
    OptionType option_type() const noexcept { return terms_.option_type(); }
    /// Returns the positive strike price.
    double strike() const noexcept { return terms_.strike(); }
    /// Returns the first date of the option life.
    Date effective_date() const noexcept { return terms_.effective_date(); }
    /// Returns the option expiry date.
    Date expiry_date() const noexcept { return terms_.expiry_date(); }
    /// Returns the validated contractual terms.
    const OptionTerms& terms() const noexcept { return terms_; }
    /// Returns the payoff tag.
    const Payoff& payoff() const noexcept { return payoff_; }
    /// Returns the exercise-style tag.
    const Exercise& exercise() const noexcept { return exercise_; }

    /// Returns the fixed payout when the payoff type provides one.
    double payout() const noexcept
        requires requires(const Payoff& value) { value.payout(); }
    {
        return payoff_.payout();
    }

    /// Compares terms, payoff, and exercise style.
    friend bool operator==(const ExerciseBasedOption&, const ExerciseBasedOption&) = default;

private:
    ExerciseBasedOption(OptionTerms terms, Payoff payoff, Exercise exercise)
        : terms_(std::move(terms)), payoff_(std::move(payoff)), exercise_(std::move(exercise)) {}

    OptionTerms terms_;
    Payoff payoff_;
    Exercise exercise_;

    template <OptionPayoff OtherPayoff, OptionExercise OtherExercise>
    friend Result<ExerciseBasedOption<OtherPayoff, OtherExercise>> detail::make_option(
        OptionType, double, Date, Date, OtherPayoff, OtherExercise);
};

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline Result<ExerciseBasedOption<Payoff, Exercise>> detail::make_option(
    OptionType option_type, double strike, Date effective_date, Date expiry_date, Payoff payoff, Exercise exercise)
{
    auto terms = detail::make_option_terms(option_type, strike, effective_date, expiry_date);
    if (!terms) return std::unexpected(terms.error());
    return ExerciseBasedOption<Payoff, Exercise>{std::move(*terms), std::move(payoff), std::move(exercise)};
}

} // namespace kiyosi
