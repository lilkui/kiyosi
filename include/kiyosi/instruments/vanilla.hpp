#pragma once

#include <kiyosi/core/types.hpp>

namespace kiyosi {

enum class option_type {
    call,
    put,
};

class EuropeanOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date expiry() const noexcept { return expiry_; }

    friend bool operator==(const EuropeanOption&, const EuropeanOption&) = default;

private:
    EuropeanOption(option_type type, double strike, date expiry)
        : type_(type), strike_(strike), expiry_(expiry) {}

    option_type type_;
    double strike_;
    date expiry_;

    friend result<EuropeanOption> make_european_option(option_type, double, date);
};

class AmericanOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date expiry() const noexcept { return expiry_; }

    friend bool operator==(const AmericanOption&, const AmericanOption&) = default;

private:
    AmericanOption(option_type type, double strike, date expiry)
        : type_(type), strike_(strike), expiry_(expiry) {}

    option_type type_;
    double strike_;
    date expiry_;

    friend result<AmericanOption> make_american_option(option_type, double, date);
};

[[nodiscard]] inline result<EuropeanOption> make_european_option(option_type type, double strike, date expiry)
{
    if (type != option_type::call && type != option_type::put) {
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    }
    if (!std::isfinite(strike) || strike <= 0.0) {
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    }
    if (!is_valid_date(expiry)) {
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    }
    return EuropeanOption{type, strike, expiry};
}

[[nodiscard]] inline result<AmericanOption> make_american_option(option_type type, double strike, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    return AmericanOption{type, strike, expiry};
}

[[nodiscard]] inline result<void> validate_expiry(date valuation_date, date expiry);
[[nodiscard]] inline result<void> validate_expiry(timestamp valuation_time, date expiry);

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

} // namespace kiyosi
