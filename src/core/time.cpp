#include <kiyosi/core/time.hpp>

namespace kiyosi {

bool is_valid_date(date value) noexcept
{
    return std::chrono::year_month_day{value}.ok();
}

result<void> validate_expiry(date valuation_date, date expiry)
{
    if (!is_valid_date(valuation_date) || !is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "valuation date and expiry must be valid calendar dates"});
    if (expiry < valuation_date)
        return std::unexpected(Error{error_category::invalid_expiry, "expiry must not precede the valuation date"});
    return {};
}

result<void> validate_expiry(timestamp valuation_time, date expiry)
{
    if (!is_valid_date(date_of(valuation_time)) || !is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "valuation time and expiry must contain valid calendar dates"});
    if (valuation_time > start_of_day(expiry))
        return std::unexpected(Error{error_category::invalid_expiry, "expiry must not precede the valuation time"});
    return {};
}

} // namespace kiyosi
