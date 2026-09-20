#pragma once

#include <cmath>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi {

enum class OptionType { call,
                        put };

class OptionTerms;

namespace detail {
[[nodiscard]] Result<OptionTerms> make_option_terms(OptionType, double, Date, Date);
}

/// Contractual essentials shared by every strike-and-life option: option_type, strike, and life dates.
class OptionTerms {
public:
    OptionType option_type() const noexcept { return option_type_; }
    double strike() const noexcept { return strike_; }
    Date effective_date() const noexcept { return effective_date_; }
    Date expiry_date() const noexcept { return expiry_date_; }
    friend bool operator==(const OptionTerms&, const OptionTerms&) = default;

private:
    OptionTerms(OptionType option_type, double strike, Date effective_date, Date expiry_date)
        : option_type_(option_type), strike_(strike), effective_date_(effective_date), expiry_date_(expiry_date) {}
    OptionType option_type_;
    double strike_;
    Date effective_date_;
    Date expiry_date_;
    friend Result<OptionTerms> detail::make_option_terms(OptionType, double, Date, Date);
};

[[nodiscard]] inline Result<OptionTerms> detail::make_option_terms(
    OptionType option_type, double strike, Date effective_date, Date expiry_date)
{
    if (option_type != OptionType::call && option_type != OptionType::put)
        return std::unexpected(Error{ErrorCategory::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_strike, "strike must be finite and positive"});
    if (!is_supported_date(effective_date) || !is_supported_date(expiry_date) || effective_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "option life dates are invalid"});
    return OptionTerms{option_type, strike, effective_date, expiry_date};
}

} // namespace kiyosi
