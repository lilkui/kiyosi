#pragma once

#include <cmath>
#include <vector>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

class AsianOptionTerms {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date average_start() const noexcept { return average_start_; }
    double realized_average() const noexcept { return realized_average_; }
    date expiry() const noexcept { return expiry_; }
    friend bool operator==(const AsianOptionTerms&, const AsianOptionTerms&) = default;

private:
    AsianOptionTerms(option_type type, double strike, date average_start, double realized_average, date expiry)
        : type_(type), strike_(strike), average_start_(average_start), realized_average_(realized_average), expiry_(expiry) {}
    option_type type_;
    double strike_;
    date average_start_;
    double realized_average_;
    date expiry_;
    friend result<AsianOptionTerms> make_asian_option_terms(option_type, double, date, double, date);
};

[[nodiscard]] inline result<AsianOptionTerms> make_asian_option_terms(
    option_type type, double strike, date average_start, double realized_average, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(realized_average) || realized_average < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "realized average must be finite and non-negative"});
    if (!is_valid_date(average_start) || !is_valid_date(expiry) || average_start > expiry)
        return std::unexpected(Error{error_category::invalid_schedule, "average dates are invalid"});
    return AsianOptionTerms{type, strike, average_start, realized_average, expiry};
}

class GeometricAverageOption {
public:
    option_type type() const noexcept { return terms_.type(); }
    double strike() const noexcept { return terms_.strike(); }
    date average_start() const noexcept { return terms_.average_start(); }
    double realized_average() const noexcept { return terms_.realized_average(); }
    date expiry() const noexcept { return terms_.expiry(); }
    friend bool operator==(const GeometricAverageOption&, const GeometricAverageOption&) = default;

private:
    explicit GeometricAverageOption(AsianOptionTerms terms) : terms_(std::move(terms)) {}
    AsianOptionTerms terms_;
    friend result<GeometricAverageOption> make_geometric_average_option(option_type, double, date, date, double);
};

class ArithmeticAverageOption {
public:
    option_type type() const noexcept { return terms_.type(); }
    double strike() const noexcept { return terms_.strike(); }
    date average_start() const noexcept { return terms_.average_start(); }
    double realized_average() const noexcept { return terms_.realized_average(); }
    date expiry() const noexcept { return terms_.expiry(); }
    friend bool operator==(const ArithmeticAverageOption&, const ArithmeticAverageOption&) = default;

private:
    explicit ArithmeticAverageOption(AsianOptionTerms terms) : terms_(std::move(terms)) {}
    AsianOptionTerms terms_;
    friend result<ArithmeticAverageOption> make_arithmetic_average_option(option_type, double, date, date, double);
};
using AsianOption = AsianOptionTerms;

[[nodiscard]] inline result<GeometricAverageOption> make_geometric_average_option(
    option_type type, double strike, date average_start, date expiry, double realized_average = 0.0)
{
    auto terms = make_asian_option_terms(type, strike, average_start, realized_average, expiry);
    if (!terms) return std::unexpected(terms.error());
    return GeometricAverageOption{std::move(*terms)};
}

[[nodiscard]] inline result<ArithmeticAverageOption> make_arithmetic_average_option(
    option_type type, double strike, date average_start, date expiry, double realized_average = 0.0)
{
    auto terms = make_asian_option_terms(type, strike, average_start, realized_average, expiry);
    if (!terms) return std::unexpected(terms.error());
    return ArithmeticAverageOption{std::move(*terms)};
}

[[nodiscard]] inline result<ArithmeticAverageOption> make_arithmetic_average_option(
    option_type type, double strike, date average_start, double realized_average, date effective, date expiry)
{
    if (average_start < effective)
        return std::unexpected(Error{error_category::invalid_schedule, "average start precedes effective date"});
    return make_arithmetic_average_option(type, strike, average_start, expiry, realized_average);
}
using GeometricAsianOption = GeometricAverageOption;
using ArithmeticAsianOption = ArithmeticAverageOption;

} // namespace kiyosi
