#include <kiyosi/core/types.hpp>

namespace kiyosi {

bool is_valid_date(date value) noexcept
{
    return std::chrono::year_month_day{value}.ok();
}

result<double> year_fraction(date start, date end, day_count_convention convention)
{
    if (!is_valid_date(start) || !is_valid_date(end))
        return std::unexpected(Error{error_category::invalid_date, "day-count dates must be valid calendar dates"});
    if (end < start)
        return std::unexpected(Error{error_category::invalid_expiry, "day-count end must not precede start"});
    if (convention != day_count_convention::actual_365_fixed)
        return std::unexpected(Error{error_category::invalid_parameter, "unsupported day-count convention"});
    return static_cast<double>((end - start).count()) / 365.0;
}

result<double> year_fraction(timestamp start, timestamp end, day_count_convention convention)
{
    if (!is_valid_date(date_of(start)) || !is_valid_date(date_of(end)))
        return std::unexpected(Error{error_category::invalid_date, "day-count timestamps must contain valid dates"});
    if (end < start)
        return std::unexpected(Error{error_category::invalid_expiry, "day-count end must not precede start"});
    if (convention != day_count_convention::actual_365_fixed)
        return std::unexpected(Error{error_category::invalid_parameter, "unsupported day-count convention"});
    return std::chrono::duration<double, std::ratio<86400 * 365>>{end - start}.count();
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
    if (date_of(valuation_time) > expiry)
        return std::unexpected(Error{error_category::invalid_expiry, "expiry must not precede the valuation time"});
    return {};
}

} // namespace kiyosi
