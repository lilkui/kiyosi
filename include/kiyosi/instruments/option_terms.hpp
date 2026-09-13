#pragma once

#include <cmath>
#include <kiyosi/core/types.hpp>

namespace kiyosi {

enum class option_type { call,
                         put };

class OptionTerms;

namespace detail {
[[nodiscard]] result<OptionTerms> make_option_terms(option_type, double, date, date);
}

class OptionTerms {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date effective() const noexcept { return effective_; }
    date expiry() const noexcept { return expiry_; }
    friend bool operator==(const OptionTerms&, const OptionTerms&) = default;

private:
    OptionTerms(option_type type, double strike, date effective, date expiry)
        : type_(type), strike_(strike), effective_(effective), expiry_(expiry) {}
    option_type type_;
    double strike_;
    date effective_;
    date expiry_;
    friend result<OptionTerms> detail::make_option_terms(option_type, double, date, date);
};

[[nodiscard]] inline result<OptionTerms> detail::make_option_terms(
    option_type type, double strike, date effective, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!is_valid_date(effective) || !is_valid_date(expiry) || effective > expiry)
        return std::unexpected(Error{error_category::invalid_schedule, "option life dates are invalid"});
    return OptionTerms{type, strike, effective, expiry};
}
} // namespace kiyosi
