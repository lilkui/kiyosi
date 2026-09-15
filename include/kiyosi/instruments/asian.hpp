#pragma once

#include <cmath>
#include <utility>

#include <kiyosi/instruments/option_terms.hpp>

namespace kiyosi {

inline constexpr double default_realized_average = 0.0;

class AsianOptionTerms;

namespace detail {
[[nodiscard]] result<AsianOptionTerms> make_asian_option_terms(
    option_type, double, date, double, date, date);
}

/// Option terms extended with the averaging window and the average realized so far.
class AsianOptionTerms {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date average_start() const noexcept { return average_start_; }
    date effective() const noexcept { return effective_; }
    double realized_average() const noexcept { return realized_average_; }
    date expiry() const noexcept { return expiry_; }
    friend bool operator==(const AsianOptionTerms&, const AsianOptionTerms&) = default;

private:
    AsianOptionTerms(option_type type, double strike, date average_start, double realized_average,
                     date effective, date expiry)
        : type_(type), strike_(strike), average_start_(average_start),
          realized_average_(realized_average), effective_(effective), expiry_(expiry) {}
    option_type type_;
    double strike_;
    date average_start_;
    double realized_average_;
    date effective_;
    date expiry_;
    friend result<AsianOptionTerms> detail::make_asian_option_terms(
        option_type, double, date, double, date, date);
};

[[nodiscard]] inline result<AsianOptionTerms> detail::make_asian_option_terms(
    option_type type, double strike, date average_start, double realized_average, date effective,
    date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(realized_average) || realized_average < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "realized average must be finite and non-negative"});
    if (!is_valid_date(average_start) || !is_valid_date(effective) || !is_valid_date(expiry) ||
        effective > expiry || average_start < effective || average_start > expiry)
        return std::unexpected(Error{error_category::invalid_schedule, "average dates are invalid"});
    return AsianOptionTerms{type, strike, average_start, realized_average, effective, expiry};
}

/// Averaging conventions; the tag selects the pricing engine overload.
struct GeometricAveraging {
    friend bool operator==(const GeometricAveraging&, const GeometricAveraging&) = default;
};

struct ArithmeticAveraging {
    friend bool operator==(const ArithmeticAveraging&, const ArithmeticAveraging&) = default;
};

template <typename Averaging>
class AverageOption;

namespace detail {
template <typename Averaging>
[[nodiscard]] result<AverageOption<Averaging>> make_average_option(AsianOptionTerms);
}

/// Average-rate option; `Averaging` distinguishes the geometric and arithmetic conventions.
template <typename Averaging>
class AverageOption {
public:
    option_type type() const noexcept { return terms_.type(); }
    double strike() const noexcept { return terms_.strike(); }
    date average_start() const noexcept { return terms_.average_start(); }
    date effective() const noexcept { return terms_.effective(); }
    double realized_average() const noexcept { return terms_.realized_average(); }
    date expiry() const noexcept { return terms_.expiry(); }
    const AsianOptionTerms& terms() const noexcept { return terms_; }
    friend bool operator==(const AverageOption&, const AverageOption&) = default;

private:
    explicit AverageOption(AsianOptionTerms terms) : terms_(std::move(terms)) {}
    AsianOptionTerms terms_;

    template <typename OtherAveraging>
    friend result<AverageOption<OtherAveraging>> detail::make_average_option(AsianOptionTerms);
};

template <typename Averaging>
[[nodiscard]] inline result<AverageOption<Averaging>> detail::make_average_option(AsianOptionTerms terms)
{
    return AverageOption<Averaging>{std::move(terms)};
}

using GeometricAverageOption = AverageOption<GeometricAveraging>;
using ArithmeticAverageOption = AverageOption<ArithmeticAveraging>;

template <typename Averaging>
[[nodiscard]] inline result<AverageOption<Averaging>> make_average_option(
    option_type type, double strike, date average_start, date effective, date expiry,
    double realized_average = default_realized_average)
{
    auto terms = detail::make_asian_option_terms(type, strike, average_start, realized_average,
                                                 effective, expiry);
    if (!terms) return std::unexpected(terms.error());
    return detail::make_average_option<Averaging>(std::move(*terms));
}

[[nodiscard]] inline result<GeometricAverageOption> make_geometric_average_option(
    option_type type, double strike, date average_start, date effective, date expiry,
    double realized_average = default_realized_average)
{
    return make_average_option<GeometricAveraging>(type, strike, average_start, effective, expiry,
                                                   realized_average);
}

[[nodiscard]] inline result<ArithmeticAverageOption> make_arithmetic_average_option(
    option_type type, double strike, date average_start, date effective, date expiry,
    double realized_average = default_realized_average)
{
    return make_average_option<ArithmeticAveraging>(type, strike, average_start, effective, expiry,
                                                    realized_average);
}

} // namespace kiyosi
