#pragma once

#include <cmath>
#include <utility>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/instruments/option_terms.hpp>

namespace kiyosi {

/// Default realized average for a contract whose averaging has not begun.
inline constexpr double default_realized_average = 0.0;

/// Averaging conventions; the tag selects the pricing engine overload.
struct GeometricAveraging {
    /// All geometric-averaging tags compare equal.
    friend bool operator==(const GeometricAveraging&, const GeometricAveraging&) = default;
};

/// Tag selecting arithmetic averaging.
struct ArithmeticAveraging {
    /// All arithmetic-averaging tags compare equal.
    friend bool operator==(const ArithmeticAveraging&, const ArithmeticAveraging&) = default;
};

template <typename Averaging>
class AveragePriceOption;

template <typename Averaging>
[[nodiscard]] Result<AveragePriceOption<Averaging>> make_average_option(
    OptionType, double, Date, Date, Date, double realized_average = default_realized_average);

/// Average-rate option; `Averaging` distinguishes the geometric and arithmetic conventions.
template <typename Averaging>
class AveragePriceOption {
public:
    /// Returns the call-or-put direction.
    OptionType option_type() const noexcept { return terms_.option_type(); }
    /// Returns the positive strike price.
    double strike() const noexcept { return terms_.strike(); }
    /// Returns the first date included in the average.
    Date averaging_start_date() const noexcept { return averaging_start_date_; }
    /// Returns the first date of the contract life.
    Date effective_date() const noexcept { return terms_.effective_date(); }
    /// Returns the non-negative average realized before valuation.
    double realized_average() const noexcept { return realized_average_; }
    /// Returns the final date of the contract life and averaging window.
    Date expiry_date() const noexcept { return terms_.expiry_date(); }
    /// Compares all average-price contract terms.
    friend bool operator==(const AveragePriceOption&, const AveragePriceOption&) = default;

private:
    AveragePriceOption(OptionTerms terms, Date averaging_start_date, double realized_average)
        : terms_(std::move(terms)), averaging_start_date_(averaging_start_date), realized_average_(realized_average) {}
    OptionTerms terms_;
    Date averaging_start_date_;
    double realized_average_;

    template <typename OtherAveraging>
    friend Result<AveragePriceOption<OtherAveraging>> make_average_option(
        OptionType, double, Date, Date, Date, double);
};

/// Average-price option using geometric averaging.
using GeometricAveragePriceOption = AveragePriceOption<GeometricAveraging>;
/// Average-price option using arithmetic averaging.
using ArithmeticAveragePriceOption = AveragePriceOption<ArithmeticAveraging>;

namespace detail {
// Requires a validated valuation strictly before expiry.
inline double arithmetic_average_adjusted_strike(const ArithmeticAveragePriceOption& option, Timestamp valuation)
{
    const double period = actual_365_fixed_year_fraction(option.averaging_start_date(), option.expiry_date());
    const double remaining = actual_365_fixed_year_fraction(valuation, option.expiry_date());
    const double elapsed = period - remaining;
    return elapsed > 0.0 ? option.strike() + elapsed / remaining * (option.strike() - option.realized_average())
                         : option.strike();
}
} // namespace detail

/// Date arguments are ordered `effective_date`, `averaging_start_date`, `expiry_date`; valid terms satisfy
/// `effective_date <= averaging_start_date <= expiry_date`.
/// Equal averaging start and expiry dates denote a single fixing at expiry.
/// @tparam Averaging GeometricAveraging or ArithmeticAveraging.
/// @return The option, or an input-validation error.
template <typename Averaging>
[[nodiscard]] inline Result<AveragePriceOption<Averaging>> make_average_option(
    OptionType option_type, double strike, Date effective_date, Date averaging_start_date, Date expiry_date,
    double realized_average)
{
    const auto direction_and_strike = detail::validate_option_direction_and_strike(option_type, strike);
    if (!direction_and_strike) return std::unexpected(direction_and_strike.error());
    if (!std::isfinite(realized_average) || realized_average < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "realized average must be finite and non-negative"});
    auto terms = detail::make_option_terms(option_type, strike, effective_date, expiry_date);
    if (!terms) return std::unexpected(terms.error());
    if (!is_supported_date(averaging_start_date))
        return std::unexpected(Error{ErrorCategory::invalid_date, "averaging start date is invalid"});
    if (averaging_start_date < effective_date || averaging_start_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "average dates are invalid"});
    return AveragePriceOption<Averaging>{std::move(*terms), averaging_start_date, realized_average};
}

/// Creates a geometric-average option; dates follow make_average_option ordering.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<GeometricAveragePriceOption> make_geometric_average_option(
    OptionType option_type, double strike, Date effective_date, Date averaging_start_date, Date expiry_date,
    double realized_average = default_realized_average)
{
    return make_average_option<GeometricAveraging>(option_type, strike, effective_date, averaging_start_date, expiry_date,
                                                   realized_average);
}

/// Creates an arithmetic-average option; dates follow make_average_option ordering.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<ArithmeticAveragePriceOption> make_arithmetic_average_option(
    OptionType option_type, double strike, Date effective_date, Date averaging_start_date, Date expiry_date,
    double realized_average = default_realized_average)
{
    return make_average_option<ArithmeticAveraging>(option_type, strike, effective_date, averaging_start_date, expiry_date,
                                                    realized_average);
}

} // namespace kiyosi
