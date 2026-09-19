#include <kiyosi/core/time.hpp>

namespace kiyosi {

bool is_valid_date(Date value) noexcept
{
    constexpr auto first_supported = Date{std::chrono::year::min() / std::chrono::January / 1};
    constexpr auto last_supported = Date{std::chrono::year::max() / std::chrono::December / 31};
    if (value < first_supported || value > last_supported) return false;
    return std::chrono::year_month_day{value}.ok();
}

Result<void> validate_valuation_not_after_expiry(Date valuation_date, Date expiry_date)
{
    if (!is_valid_date(valuation_date) || !is_valid_date(expiry_date))
        return std::unexpected(Error{ErrorCategory::invalid_date, "valuation date and expiry date must be valid calendar dates"});
    if (expiry_date < valuation_date)
        return std::unexpected(Error{ErrorCategory::invalid_expiry, "expiry date must not precede the valuation date"});
    return {};
}

Result<void> validate_valuation_not_after_expiry(Timestamp valuation_time, Date expiry_date)
{
    if (!is_valid_date(date_of(valuation_time)) || !is_valid_date(expiry_date))
        return std::unexpected(Error{ErrorCategory::invalid_date, "valuation time and expiry date must contain valid calendar dates"});
    if (valuation_time > start_of_day(expiry_date))
        return std::unexpected(Error{ErrorCategory::invalid_expiry, "expiry date must not precede the valuation time"});
    return {};
}

} // namespace kiyosi
