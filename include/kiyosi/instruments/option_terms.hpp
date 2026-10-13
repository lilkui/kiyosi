#pragma once

#include <cstdint>

#include <cmath>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi {

/// Direction of an option payoff.
enum class OptionType : std::uint8_t {
    call, ///< Right to benefit from prices above the strike.
    put   ///< Right to benefit from prices below the strike.
};

class OptionTerms;

namespace detail {
[[nodiscard]] Result<OptionTerms> make_option_terms(OptionType, double, Date, Date);

[[nodiscard]] inline Result<void> validate_option_direction_and_strike(OptionType option_type, double strike)
{
    if (option_type != OptionType::call && option_type != OptionType::put)
        return std::unexpected(Error{ErrorCategory::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_strike, "strike must be finite and positive"});
    return {};
}
}

/// Contractual essentials shared by every strike-and-life option: option_type, strike, and life dates.
class OptionTerms {
public:
    /// Returns the call-or-put direction.
    OptionType option_type() const noexcept { return option_type_; }
    /// Returns the positive strike price.
    double strike() const noexcept { return strike_; }
    /// Returns the first date of the contract life.
    Date effective_date() const noexcept { return effective_date_; }
    /// Returns the final date of the contract life.
    Date expiry_date() const noexcept { return expiry_date_; }
    /// Compares all contractual terms.
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
    const auto direction_and_strike = validate_option_direction_and_strike(option_type, strike);
    if (!direction_and_strike) return std::unexpected(direction_and_strike.error());
    auto life = validate_instrument_life(effective_date, expiry_date);
    if (!life) return std::unexpected(life.error());
    return OptionTerms{option_type, strike, effective_date, expiry_date};
}

} // namespace kiyosi
