#pragma once

#include <cmath>
#include <utility>

#include <kiyosi/instruments/option_terms.hpp>

namespace kiyosi {

inline constexpr double default_realized_average = 0.0;

class AveragePriceOptionTerms;

namespace detail {
[[nodiscard]] Result<AveragePriceOptionTerms> make_asian_option_terms(
    OptionType, double, Date, double, Date, Date);
}

/// Option terms extended with the averaging window and the average realized so far.
class AveragePriceOptionTerms {
public:
    OptionType option_type() const noexcept { return option_type_; }
    double strike() const noexcept { return strike_; }
    Date averaging_start_date() const noexcept { return averaging_start_date_; }
    Date effective_date() const noexcept { return effective_date_; }
    double realized_average() const noexcept { return realized_average_; }
    Date expiry_date() const noexcept { return expiry_date_; }
    friend bool operator==(const AveragePriceOptionTerms&, const AveragePriceOptionTerms&) = default;

private:
    AveragePriceOptionTerms(OptionType option_type, double strike, Date averaging_start_date, double realized_average,
                            Date effective_date, Date expiry_date)
        : option_type_(option_type), strike_(strike), averaging_start_date_(averaging_start_date),
          realized_average_(realized_average), effective_date_(effective_date), expiry_date_(expiry_date) {}
    OptionType option_type_;
    double strike_;
    Date averaging_start_date_;
    double realized_average_;
    Date effective_date_;
    Date expiry_date_;
    friend Result<AveragePriceOptionTerms> detail::make_asian_option_terms(
        OptionType, double, Date, double, Date, Date);
};

[[nodiscard]] inline Result<AveragePriceOptionTerms> detail::make_asian_option_terms(
    OptionType option_type, double strike, Date averaging_start_date, double realized_average, Date effective_date,
    Date expiry_date)
{
    if (option_type != OptionType::call && option_type != OptionType::put)
        return std::unexpected(Error{ErrorCategory::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(realized_average) || realized_average < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "realized average must be finite and non-negative"});
    if (!is_supported_date(averaging_start_date) || !is_supported_date(effective_date) || !is_supported_date(expiry_date) ||
        effective_date > expiry_date || averaging_start_date < effective_date || averaging_start_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "average dates are invalid"});
    return AveragePriceOptionTerms{option_type, strike, averaging_start_date, realized_average, effective_date, expiry_date};
}

/// Averaging conventions; the tag selects the pricing engine overload.
struct GeometricAveraging {
    friend bool operator==(const GeometricAveraging&, const GeometricAveraging&) = default;
};

struct ArithmeticAveraging {
    friend bool operator==(const ArithmeticAveraging&, const ArithmeticAveraging&) = default;
};

template <typename Averaging>
class AveragePriceOption;

namespace detail {
template <typename Averaging>
[[nodiscard]] Result<AveragePriceOption<Averaging>> make_average_option(AveragePriceOptionTerms);
}

/// Average-rate option; `Averaging` distinguishes the geometric and arithmetic conventions.
template <typename Averaging>
class AveragePriceOption {
public:
    OptionType option_type() const noexcept { return terms_.option_type(); }
    double strike() const noexcept { return terms_.strike(); }
    Date averaging_start_date() const noexcept { return terms_.averaging_start_date(); }
    Date effective_date() const noexcept { return terms_.effective_date(); }
    double realized_average() const noexcept { return terms_.realized_average(); }
    Date expiry_date() const noexcept { return terms_.expiry_date(); }
    const AveragePriceOptionTerms& terms() const noexcept { return terms_; }
    friend bool operator==(const AveragePriceOption&, const AveragePriceOption&) = default;

private:
    explicit AveragePriceOption(AveragePriceOptionTerms terms) : terms_(std::move(terms)) {}
    AveragePriceOptionTerms terms_;

    template <typename OtherAveraging>
    friend Result<AveragePriceOption<OtherAveraging>> detail::make_average_option(AveragePriceOptionTerms);
};

template <typename Averaging>
[[nodiscard]] inline Result<AveragePriceOption<Averaging>> detail::make_average_option(AveragePriceOptionTerms terms)
{
    return AveragePriceOption<Averaging>{std::move(terms)};
}

using GeometricAveragePriceOption = AveragePriceOption<GeometricAveraging>;
using ArithmeticAveragePriceOption = AveragePriceOption<ArithmeticAveraging>;

/// Date arguments are ordered `averaging_start_date`, `effective_date`, `expiry_date`; valid terms satisfy
/// `effective_date <= averaging_start_date <= expiry_date`.
template <typename Averaging>
[[nodiscard]] inline Result<AveragePriceOption<Averaging>> make_average_option(
    OptionType option_type, double strike, Date averaging_start_date, Date effective_date, Date expiry_date,
    double realized_average = default_realized_average)
{
    auto terms = detail::make_asian_option_terms(option_type, strike, averaging_start_date, realized_average,
                                                 effective_date, expiry_date);
    if (!terms) return std::unexpected(terms.error());
    return detail::make_average_option<Averaging>(std::move(*terms));
}

/// Geometric-average option factory; Date arguments follow make_average_option ordering.
[[nodiscard]] inline Result<GeometricAveragePriceOption> make_geometric_average_option(
    OptionType option_type, double strike, Date averaging_start_date, Date effective_date, Date expiry_date,
    double realized_average = default_realized_average)
{
    return make_average_option<GeometricAveraging>(option_type, strike, averaging_start_date, effective_date, expiry_date,
                                                   realized_average);
}

/// Arithmetic-average option factory; Date arguments follow make_average_option ordering.
[[nodiscard]] inline Result<ArithmeticAveragePriceOption> make_arithmetic_average_option(
    OptionType option_type, double strike, Date averaging_start_date, Date effective_date, Date expiry_date,
    double realized_average = default_realized_average)
{
    return make_average_option<ArithmeticAveraging>(option_type, strike, averaging_start_date, effective_date, expiry_date,
                                                    realized_average);
}

} // namespace kiyosi
