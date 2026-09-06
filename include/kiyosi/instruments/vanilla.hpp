#pragma once

#include <cmath>
#include <concepts>
#include <span>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/core/types.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi {

enum class option_type { call, put };

class OptionTerms {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date expiry() const noexcept { return expiry_; }
    friend bool operator==(const OptionTerms&, const OptionTerms&) = default;

private:
    OptionTerms(option_type type, double strike, date expiry)
        : type_(type), strike_(strike), expiry_(expiry) {}
    option_type type_;
    double strike_;
    date expiry_;
    friend result<OptionTerms> make_option_terms(option_type, double, date);
};

[[nodiscard]] inline result<OptionTerms> make_option_terms(option_type type, double strike, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    return OptionTerms{type, strike, expiry};
}

struct VanillaPayoff {
    friend bool operator==(const VanillaPayoff&, const VanillaPayoff&) = default;
};

class CashOrNothingPayoff {
public:
    double payout() const noexcept { return payout_; }
    friend bool operator==(const CashOrNothingPayoff&, const CashOrNothingPayoff&) = default;

private:
    explicit CashOrNothingPayoff(double payout) : payout_(payout) {}
    double payout_;
    friend result<CashOrNothingPayoff> make_cash_or_nothing_payoff(double);
};

[[nodiscard]] inline result<CashOrNothingPayoff> make_cash_or_nothing_payoff(double payout)
{
    if (!std::isfinite(payout) || payout <= 0.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "payout must be finite and positive"});
    return CashOrNothingPayoff{payout};
}

struct AssetOrNothingPayoff {
    friend bool operator==(const AssetOrNothingPayoff&, const AssetOrNothingPayoff&) = default;
};

struct EuropeanExercise {
    friend bool operator==(const EuropeanExercise&, const EuropeanExercise&) = default;
};

struct AmericanExercise {
    friend bool operator==(const AmericanExercise&, const AmericanExercise&) = default;
};

class BermudanExercise {
public:
    const std::vector<date>& dates() const noexcept { return dates_; }
    const std::vector<date>& exercise_dates() const noexcept { return dates_; }
    const std::vector<date>& observation_dates() const noexcept { return dates_; }
    std::size_t size() const noexcept { return dates_.size(); }
    bool empty() const noexcept { return dates_.empty(); }
    friend bool operator==(const BermudanExercise&, const BermudanExercise&) = default;

private:
    explicit BermudanExercise(std::vector<date> dates) : dates_(std::move(dates)) {}
    std::vector<date> dates_;
    friend result<BermudanExercise> make_bermudan_exercise(
        std::vector<date>, date, const TradingCalendar&);
};

[[nodiscard]] inline result<BermudanExercise> make_bermudan_exercise(
    std::vector<date> dates, date expiry, const TradingCalendar& calendar = all_days_calendar())
{
    if (dates.empty())
        return std::unexpected(Error{error_category::invalid_schedule,
                                     "Bermudan exercise requires at least one date"});
    auto valid = validate_observation_dates(dates, dates.front(), expiry, calendar);
    if (!valid)
        return std::unexpected(Error{error_category::invalid_schedule, valid.error().message});
    return BermudanExercise{std::move(dates)};
}

template <typename Value>
concept OptionPayoff = std::same_as<std::remove_cvref_t<Value>, VanillaPayoff> ||
                       std::same_as<std::remove_cvref_t<Value>, CashOrNothingPayoff> ||
                       std::same_as<std::remove_cvref_t<Value>, AssetOrNothingPayoff>;

template <typename Value>
concept OptionExercise = std::same_as<std::remove_cvref_t<Value>, EuropeanExercise> ||
                         std::same_as<std::remove_cvref_t<Value>, AmericanExercise> ||
                         std::same_as<std::remove_cvref_t<Value>, BermudanExercise>;

template <OptionPayoff Payoff, OptionExercise Exercise>
class ExerciseBasedOption;

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] result<ExerciseBasedOption<Payoff, Exercise>> make_exercise_based_option(
    OptionTerms, Payoff, Exercise);

template <OptionPayoff Payoff, OptionExercise Exercise>
class ExerciseBasedOption {
public:
    option_type type() const noexcept { return terms_.type(); }
    double strike() const noexcept { return terms_.strike(); }
    date expiry() const noexcept { return terms_.expiry(); }
    const OptionTerms& terms() const noexcept { return terms_; }
    const Payoff& payoff() const noexcept { return payoff_; }
    const Exercise& exercise() const noexcept { return exercise_; }

    double payout() const noexcept requires requires(const Payoff& value) { value.payout(); }
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
    friend result<ExerciseBasedOption<OtherPayoff, OtherExercise>> make_exercise_based_option(
        OptionTerms, OtherPayoff, OtherExercise);
};

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, Exercise>> make_exercise_based_option(
    OptionTerms terms, Payoff payoff, Exercise exercise)
{
    if constexpr (std::same_as<Exercise, BermudanExercise>) {
        auto valid = validate_observation_dates(
            exercise.dates(), exercise.dates().front(), terms.expiry(), all_days_calendar());
        if (!valid)
            return std::unexpected(Error{error_category::invalid_schedule, valid.error().message});
    }
    return ExerciseBasedOption<Payoff, Exercise>{std::move(terms), std::move(payoff), std::move(exercise)};
}

template <OptionPayoff Payoff>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, EuropeanExercise>> make_european_option(
    OptionTerms terms, Payoff payoff)
{
    return make_exercise_based_option(std::move(terms), std::move(payoff), EuropeanExercise{});
}

template <OptionPayoff Payoff>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, AmericanExercise>> make_american_option(
    OptionTerms terms, Payoff payoff)
{
    return make_exercise_based_option(std::move(terms), std::move(payoff), AmericanExercise{});
}

template <OptionPayoff Payoff>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, BermudanExercise>> make_bermudan_option(
    OptionTerms terms, Payoff payoff, std::vector<date> dates,
    const TradingCalendar& calendar = all_days_calendar())
{
    auto exercise = make_bermudan_exercise(std::move(dates), terms.expiry(), calendar);
    if (!exercise) return std::unexpected(exercise.error());
    return make_exercise_based_option(std::move(terms), std::move(payoff), std::move(*exercise));
}

template <OptionPayoff Payoff, OptionExercise Exercise>
[[nodiscard]] inline result<ExerciseBasedOption<Payoff, Exercise>> make_option(
    option_type type, double strike, date expiry, Payoff payoff, Exercise exercise)
{
    auto terms = make_option_terms(type, strike, expiry);
    if (!terms) return std::unexpected(terms.error());
    return make_exercise_based_option(*terms, std::move(payoff), std::move(exercise));
}

using EuropeanOption = ExerciseBasedOption<VanillaPayoff, EuropeanExercise>;
using AmericanOption = ExerciseBasedOption<VanillaPayoff, AmericanExercise>;
using BermudanOption = ExerciseBasedOption<VanillaPayoff, BermudanExercise>;
using EuropeanCashOrNothingOption = ExerciseBasedOption<CashOrNothingPayoff, EuropeanExercise>;
using AmericanCashOrNothingOption = ExerciseBasedOption<CashOrNothingPayoff, AmericanExercise>;
using BermudanCashOrNothingOption = ExerciseBasedOption<CashOrNothingPayoff, BermudanExercise>;
using EuropeanAssetOrNothingOption = ExerciseBasedOption<AssetOrNothingPayoff, EuropeanExercise>;
using AmericanAssetOrNothingOption = ExerciseBasedOption<AssetOrNothingPayoff, AmericanExercise>;
using BermudanAssetOrNothingOption = ExerciseBasedOption<AssetOrNothingPayoff, BermudanExercise>;

[[nodiscard]] inline result<EuropeanOption> make_european_option(option_type type, double strike, date expiry)
{
    return make_option(type, strike, expiry, VanillaPayoff{}, EuropeanExercise{});
}

[[nodiscard]] inline result<AmericanOption> make_american_option(option_type type, double strike, date expiry)
{
    return make_option(type, strike, expiry, VanillaPayoff{}, AmericanExercise{});
}

[[nodiscard]] inline result<BermudanOption> make_bermudan_option(
    option_type type, double strike, date expiry, std::vector<date> dates,
    const TradingCalendar& calendar = all_days_calendar())
{
    auto terms = make_option_terms(type, strike, expiry);
    if (!terms) return std::unexpected(terms.error());
    return make_bermudan_option(*terms, VanillaPayoff{}, std::move(dates), calendar);
}

[[nodiscard]] inline result<BermudanOption> make_bermudan_option(
    option_type type, double strike, date valuation_date, date expiry, std::vector<date> dates,
    const TradingCalendar& calendar = all_days_calendar())
{
    auto valid = validate_expiry(valuation_date, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_bermudan_option(type, strike, expiry, std::move(dates), calendar);
}

[[nodiscard]] inline result<EuropeanOption> make_european_option(
    option_type type, double strike, date valuation_date, date expiry)
{
    auto valid = validate_expiry(valuation_date, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_european_option(type, strike, expiry);
}

[[nodiscard]] inline result<AmericanOption> make_american_option(
    option_type type, double strike, date valuation_date, date expiry)
{
    auto valid = validate_expiry(valuation_date, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_american_option(type, strike, expiry);
}

[[nodiscard]] inline result<EuropeanOption> make_european_call(double strike, date expiry)
{
    return make_european_option(option_type::call, strike, expiry);
}
[[nodiscard]] inline result<EuropeanOption> make_european_put(double strike, date expiry)
{
    return make_european_option(option_type::put, strike, expiry);
}
[[nodiscard]] inline result<AmericanOption> make_american_call(double strike, date expiry)
{
    return make_american_option(option_type::call, strike, expiry);
}
[[nodiscard]] inline result<AmericanOption> make_american_put(double strike, date expiry)
{
    return make_american_option(option_type::put, strike, expiry);
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
